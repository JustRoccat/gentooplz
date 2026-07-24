use crate::model::BuildSystem;
use once_cell::sync::Lazy;
use regex::Regex;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const TAIL_BYTES: i64 = 4096;

static NINJA_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\s*(\d+)/(\d+)\]\s*(.*)").unwrap());

static MAKE_PCT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\s*(\d{1,3})%\]\s*(.*)").unwrap());

static CARGO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*Compiling\s+(\S+)\s+v?(\S*)").unwrap());
static CARGO_COUNT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\((\d+)/(\d+)\)").unwrap());

static EMERGE_JOB_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r">>>\s*emerge\s*\((\d+)\s+of\s+(\d+)\)").unwrap());

const EMERGE_LOG_TAIL_BYTES: i64 = 16_384;

#[derive(Debug, Clone, Default)]
pub struct ParsedProgress {
    pub build_system: BuildSystem,
    pub progress_pct: Option<f32>,
    pub current_step: Option<String>,
    pub recent_lines: Vec<String>,
}

fn read_tail_n(path: &Path, tail_bytes: i64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len() as i64;
    let offset = -(tail_bytes.min(len));

    if len > 0 {
        file.seek(SeekFrom::End(offset))?;
    }

    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_tail(path: &Path) -> std::io::Result<String> {
    read_tail_n(path, TAIL_BYTES)
}

fn last_meaningful_lines(text: &str, n: usize) -> Vec<&str> {
    text.lines()
        .rev()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(n)
        .collect()
}

pub fn parse_build_log(path: &Path) -> Option<ParsedProgress> {
    let text = read_tail(path).ok()?;
    let lines = last_meaningful_lines(&text, 25);
    let recent_lines: Vec<String> = lines.iter().take(8).map(|s| s.to_string()).collect();

    for line in &lines {
        if let Some(caps) = NINJA_RE.captures(line) {
            let done: f32 = caps[1].parse().unwrap_or(0.0);
            let total: f32 = caps[2].parse::<f32>().unwrap_or(1.0).max(1.0);
            return Some(ParsedProgress {
                build_system: BuildSystem::Ninja,
                progress_pct: Some((done / total) * 100.0),
                current_step: Some(caps[3].trim().to_string()),
                recent_lines,
            });
        }

        if let Some(caps) = MAKE_PCT_RE.captures(line) {
            let pct: f32 = caps[1].parse().unwrap_or(0.0);
            return Some(ParsedProgress {
                build_system: BuildSystem::Make,
                progress_pct: Some(pct.clamp(0.0, 100.0)),
                current_step: Some(caps[2].trim().to_string()),
                recent_lines,
            });
        }

        if let Some(caps) = CARGO_RE.captures(line) {
            let crate_name = caps[1].to_string();
            let version = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let step = if version.is_empty() {
                crate_name
            } else {
                format!("{crate_name} v{version}")
            };

            let pct = CARGO_COUNT_RE.captures(line).and_then(|c| {
                let done: f32 = c[1].parse().ok()?;
                let total: f32 = c[2].parse().ok()?;
                if total > 0.0 {
                    Some((done / total) * 100.0)
                } else {
                    None
                }
            });

            return Some(ParsedProgress {
                build_system: BuildSystem::Cargo,
                progress_pct: pct,
                current_step: Some(step),
                recent_lines,
            });
        }
    }

    Some(ParsedProgress {
        build_system: BuildSystem::Unknown,
        progress_pct: None,
        current_step: lines.first().map(|s| s.to_string()),
        recent_lines,
    })
}

pub fn parse_emerge_job_progress(path: &Path) -> Option<(usize, usize)> {
    let text = read_tail_n(path, EMERGE_LOG_TAIL_BYTES).ok()?;
    text.lines().rev().find_map(|line| {
        let caps = EMERGE_JOB_RE.captures(line)?;
        let current: usize = caps[1].parse().ok()?;
        let total: usize = caps[2].parse().ok()?;
        Some((current, total))
    })
}
