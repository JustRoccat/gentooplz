use crate::model::{BuildSystem, PackageState, ScanEvent, SystemCompileState};
use crate::parser::{parse_build_log, parse_emerge_job_progress};
use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::sync::mpsc::Sender;
use tokio::time::interval;

struct RawCandidate {
    category: String,
    name: String,
    version: String,
    build_dir: PathBuf,
}

static PKG_VERSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?P<name>.+)-(?P<version>[0-9]+(?:\.[0-9]+)*[a-z]?(?:_(?:alpha|beta|pre|rc|p)[0-9]*)*(?:-r[0-9]+)?)$",
    )
    .unwrap()
});

fn split_name_version(dirname: &str) -> (String, String) {
    if let Some(caps) = PKG_VERSION_RE.captures(dirname) {
        let name = caps["name"].to_string();
        let version = caps["version"].to_string();
        return (name, version);
    }

    (dirname.to_string(), String::new())
}

fn find_active_candidates(root: &Path) -> Vec<RawCandidate> {
    let mut out = Vec::new();

    let Ok(categories) = fs::read_dir(root) else {
        return out;
    };

    for category_entry in categories.flatten() {
        let category_path = category_entry.path();
        if !category_path.is_dir() {
            continue;
        }
        let Some(category) = category_entry.file_name().to_str().map(str::to_string) else {
            continue;
        };

        let Ok(packages) = fs::read_dir(&category_path) else {
            continue;
        };

        for pkg_entry in packages.flatten() {
            let build_dir = pkg_entry.path();
            if !build_dir.is_dir() {
                continue;
            }
            if !build_dir.join("temp").is_dir() {
                continue;
            }

            let Some(dirname) = pkg_entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let (name, version) = split_name_version(&dirname);

            out.push(RawCandidate {
                category: category.clone(),
                name,
                version,
                build_dir,
            });
        }
    }

    out
}

fn process_matches(build_dir: &Path, needle: &str, process: &sysinfo::Process) -> bool {
    if let Some(cwd) = process.cwd() {
        if cwd.starts_with(build_dir) {
            return true;
        }
    }
    process
        .cmd()
        .iter()
        .any(|arg| arg.to_string_lossy().contains(needle))
}

fn find_matching_pids(build_dir: &Path, sys: &System) -> Vec<Pid> {
    let needle = build_dir.to_string_lossy().into_owned();
    sys.processes()
        .iter()
        .filter(|(_, process)| process_matches(build_dir, &needle, process))
        .map(|(pid, _)| *pid)
        .collect()
}

fn sum_usage(root_pids: &[Pid], sys: &System) -> (f32, u64, usize, Option<Pid>) {
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, process) in sys.processes() {
        if let Some(parent) = process.parent() {
            children.entry(parent).or_default().push(*pid);
        }
    }

    let mut seen: std::collections::HashSet<Pid> = std::collections::HashSet::new();
    let mut stack: Vec<Pid> = root_pids.to_vec();
    let mut cpu = 0.0f32;
    let mut mem = 0u64;
    let mut top: Option<(Pid, f32)> = None;

    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(process) = sys.process(pid) {
            let p_cpu = process.cpu_usage();
            cpu += p_cpu;
            mem += process.memory();
            if top.map(|(_, c)| p_cpu > c).unwrap_or(true) {
                top = Some((pid, p_cpu));
            }
        }
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }

    (cpu, mem / 1024 / 1024, seen.len(), top.map(|(pid, _)| pid))
}

const FS_SCAN_CAP: usize = 20_000;
const SOURCE_EXTS: &[&str] = &[
    "c", "cc", "cpp", "cxx", "m", "mm", "rs", "go", "f90", "f", "java", "swift", "d",
];
const OBJECT_EXTS: &[&str] = &["o", "obj", "lo", "rlib", "rmeta", "class"];

fn count_files_with_ext(dir: &Path, exts: &[&str], cap: usize) -> usize {
    let mut count = 0usize;
    let mut visited = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > cap {
                return count;
            }

            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if exts.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                    count += 1;
                }
            }
        }
    }
    count
}

const BUILD_SYSTEM_MARKERS: &[(&str, BuildSystem)] = &[
    ("Cargo.toml", BuildSystem::Cargo),
    ("build.ninja", BuildSystem::Ninja),
    ("Makefile", BuildSystem::Make),
    ("GNUmakefile", BuildSystem::Make),
];

fn find_build_system_marker(dir: &Path, cap: usize) -> Option<BuildSystem> {
    let mut visited = 0usize;
    let mut stack = vec![dir.to_path_buf()];

    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };

        for entry in entries.flatten() {
            visited += 1;
            if visited > cap {
                return None;
            }

            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }

            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            if file_type.is_dir() {
                if file_name == "temp" {
                    continue;
                }
                stack.push(path);
                continue;
            }

            if let Some((_, system)) = BUILD_SYSTEM_MARKERS
                .iter()
                .find(|(marker, _)| *marker == file_name)
            {
                return Some(*system);
            }
        }
    }

    None
}

fn detect_build_system_hint(build_dir: &Path) -> BuildSystem {
    find_build_system_marker(build_dir, FS_SCAN_CAP).unwrap_or(BuildSystem::Unknown)
}

fn scan_once(
    sys: &mut System,
    started_at_cache: &mut HashMap<PathBuf, Instant>,
    source_count_cache: &mut HashMap<PathBuf, usize>,
    progress_cache: &mut HashMap<PathBuf, f32>,
    portage_tmp: &Path,
    emerge_log: &Path,
) -> SystemCompileState {
    sys.refresh_cpu_usage();

    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new()
            .with_cpu()
            .with_memory()
            .with_cwd(UpdateKind::Always)
            .with_cmd(UpdateKind::Always),
    );
    sys.refresh_memory();

    let candidates = find_active_candidates(portage_tmp);
    let mut active_packages = Vec::with_capacity(candidates.len());

    for c in candidates {
        let log_path = c.build_dir.join("temp").join("build.log");
        let parsed = parse_build_log(&log_path);

        let root_pids = find_matching_pids(&c.build_dir, sys);
        let (cpu_usage, memory_mb, process_count, top_pid) = sum_usage(&root_pids, sys);
        let pid = top_pid.map(|p| p.as_u32());

        let mut progress_pct = parsed.as_ref().and_then(|p| p.progress_pct);
        if progress_pct.is_none() {
            let sources = *source_count_cache
                .entry(c.build_dir.clone())
                .or_insert_with(|| count_files_with_ext(&c.build_dir, SOURCE_EXTS, FS_SCAN_CAP));
            if sources > 0 {
                let objects = count_files_with_ext(&c.build_dir, OBJECT_EXTS, FS_SCAN_CAP);

                progress_pct = Some(((objects as f32 / sources as f32) * 100.0).min(99.0));
            }
        }

        let best_so_far = progress_cache.get(&c.build_dir).copied();
        progress_pct = match (progress_pct, best_so_far) {
            (Some(new), Some(prev)) => Some(new.max(prev)),
            (Some(new), None) => Some(new),
            (None, prev) => prev,
        };
        if let Some(p) = progress_pct {
            progress_cache.insert(c.build_dir.clone(), p);
        }

        let started_at = *started_at_cache
            .entry(c.build_dir.clone())
            .or_insert_with(Instant::now);

        active_packages.push(PackageState {
            category: c.category,
            name: c.name,
            version: c.version,
            build_system: parsed
                .as_ref()
                .map(|p| p.build_system)
                .unwrap_or_else(|| detect_build_system_hint(&c.build_dir)),
            current_step: parsed.as_ref().and_then(|p| p.current_step.clone()),
            recent_lines: parsed
                .as_ref()
                .map(|p| p.recent_lines.clone())
                .unwrap_or_default(),
            progress_pct,
            memory_mb,
            cpu_usage,
            started_at,
            pid,
            process_count,
            build_dir: c.build_dir,
        });
    }

    let alive: std::collections::HashSet<_> = active_packages
        .iter()
        .map(|p| p.build_dir.clone())
        .collect();
    started_at_cache.retain(|k, _| alive.contains(k));
    source_count_cache.retain(|k, _| alive.contains(k));
    progress_cache.retain(|k, _| alive.contains(k));

    active_packages.sort_by(|a, b| a.full_name().cmp(&b.full_name()));

    let (current_job, total_jobs) =
        parse_emerge_job_progress(emerge_log).unwrap_or((0, active_packages.len()));

    SystemCompileState {
        current_job,
        total_jobs,
        system_cpu_pct: sys.global_cpu_usage(),
        system_mem_used_mb: sys.used_memory() / 1024 / 1024,
        system_mem_total_mb: sys.total_memory() / 1024 / 1024,
        load_avg_1: System::load_average().one,
        active_packages,
    }
}

pub async fn run_scanner_loop(
    tx: Sender<ScanEvent>,
    interval_ms: u64,
    portage_tmp: PathBuf,
    emerge_log: PathBuf,
) -> Result<()> {
    let mut sys = System::new_all();
    let mut started_at_cache: HashMap<PathBuf, Instant> = HashMap::new();
    let mut source_count_cache: HashMap<PathBuf, usize> = HashMap::new();
    let mut progress_cache: HashMap<PathBuf, f32> = HashMap::new();
    let mut ticker = interval(Duration::from_millis(interval_ms));

    loop {
        ticker.tick().await;

        let root = portage_tmp.clone();
        let log_path = emerge_log.clone();

        let state = tokio::task::spawn_blocking(move || {
            let mut sys = sys;
            let mut cache = started_at_cache;
            let mut src_cache = source_count_cache;
            let mut prog_cache = progress_cache;
            let state = scan_once(
                &mut sys,
                &mut cache,
                &mut src_cache,
                &mut prog_cache,
                &root,
                &log_path,
            );
            (sys, cache, src_cache, prog_cache, state)
        })
        .await;

        match state {
            Ok((sys_back, cache_back, src_cache_back, prog_cache_back, state)) => {
                sys = sys_back;
                started_at_cache = cache_back;
                source_count_cache = src_cache_back;
                progress_cache = prog_cache_back;
                if tx.send(ScanEvent::Update(state)).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                let _ = tx.send(ScanEvent::Error(format!("scan panic: {e}"))).await;
                break;
            }
        }
    }

    Ok(())
}
