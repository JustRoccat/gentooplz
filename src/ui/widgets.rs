use crate::model::{BuildSystem, PackageState, SystemCompileState};
use crate::View;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;
use std::collections::VecDeque;

const INK: Color = Color::Rgb(20, 21, 32);
const TEXT: Color = Color::Rgb(199, 206, 234);
const DIM: Color = Color::Rgb(120, 128, 160);
const FAINT: Color = Color::Rgb(58, 63, 92);
const BORDER: Color = Color::Rgb(46, 50, 74);
const HIGHLIGHT_BG: Color = Color::Rgb(35, 38, 58);

const ACCENT: Color = Color::Rgb(122, 212, 231);
const BLUE: Color = Color::Rgb(122, 162, 247);
const GREEN: Color = Color::Rgb(158, 206, 106);
const MAGENTA: Color = Color::Rgb(187, 154, 247);
const ORANGE: Color = Color::Rgb(255, 158, 100);
const RED: Color = Color::Rgb(247, 118, 142);

const NAME_W: usize = 17;
const VER_W: usize = 8;

fn build_system_meta(bs: BuildSystem) -> (char, &'static str, Color) {
    match bs {
        BuildSystem::Ninja => ('▲', "ninja", BLUE),
        BuildSystem::Make => ('■', "make", GREEN),
        BuildSystem::Cargo => ('●', "cargo", MAGENTA),
        BuildSystem::Unknown => ('○', "build", DIM),
    }
}

fn fmt_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn pad(s: &str, w: usize) -> String {
    format!("{:<w$}", s, w = w)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn centered(text: impl Into<String>, style: Style) -> Line<'static> {
    Line::from(Span::styled(text.into(), style)).alignment(Alignment::Center)
}

fn vcenter<'a>(lines: Vec<Line<'a>>, area_height: u16) -> Vec<Line<'a>> {
    let pad = (area_height as usize).saturating_sub(lines.len()) / 2;
    let mut out = vec![Line::from(""); pad];
    out.extend(lines);
    out
}

fn gauge_color(pct: f32) -> Color {
    if pct >= 85.0 {
        RED
    } else if pct >= 55.0 {
        ORANGE
    } else {
        GREEN
    }
}

fn gauge_bar(pct: f32, width: usize) -> (String, Color) {
    let pct = pct.clamp(0.0, 100.0);
    let filled = ((pct / 100.0) * width as f32).round() as usize;
    let bar = format!(
        "{}{}",
        "█".repeat(filled.min(width)),
        "░".repeat(width.saturating_sub(filled))
    );
    (bar, gauge_color(pct))
}

fn panel(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        ))
}

fn pill(text: String, color: Color) -> Span<'static> {
    Span::styled(
        format!(" {text} "),
        Style::default()
            .fg(INK)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

pub fn render_header(frame: &mut Frame, area: Rect, view: View) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(10), Constraint::Min(10)])
        .split(area);

    let logo = Line::from(vec![Span::styled(
        " gentooplz ",
        Style::default()
            .fg(INK)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD),
    )]);
    frame.render_widget(Paragraph::new(logo), cols[0]);

    let mut spans = Vec::new();
    for (i, v) in View::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(FAINT)));
        }
        if *v == view {
            spans.push(pill(v.label().to_string(), ACCENT));
        } else {
            spans.push(Span::styled(v.label(), Style::default().fg(DIM)));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Right),
        cols[1],
    );
}

pub fn render_rule(frame: &mut Frame, area: Rect) {
    let line = Paragraph::new(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(FAINT),
    ));
    frame.render_widget(line, area);
}

pub fn render_sidebar(frame: &mut Frame, area: Rect, state: &SystemCompileState, selected: usize) {
    let n = state.active_packages.len();
    let title = match (n > 0, state.total_jobs > 0) {
        (true, true) => format!(
            "packages · {}/{n} · job {}/{}",
            selected + 1,
            state.current_job,
            state.total_jobs
        ),
        (true, false) => format!("packages · {}/{n}", selected + 1),
        (false, _) => "packages".to_string(),
    };
    let block = panel(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    if n == 0 {
        lines.push(Line::from(Span::styled(
            "nothing compiling",
            Style::default().fg(FAINT).add_modifier(Modifier::ITALIC),
        )));
    } else {
        for (i, pkg) in state.active_packages.iter().enumerate() {
            lines.push(sidebar_row(pkg, i == selected));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "╌".repeat(inner.width as usize),
        Style::default().fg(FAINT),
    )));
    lines.push(Line::from(""));

    let mem_pct = if state.system_mem_total_mb > 0 {
        (state.system_mem_used_mb as f32 / state.system_mem_total_mb as f32) * 100.0
    } else {
        0.0
    };
    lines.push(sidebar_gauge(
        "cpu",
        state.system_cpu_pct,
        format!("{:.0}%", state.system_cpu_pct),
    ));
    lines.push(sidebar_gauge("ram", mem_pct, format!("{mem_pct:.0}%")));
    lines.push(sidebar_stat("load", format!("{:.2}", state.load_avg_1)));

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            (0..lines.len().min(inner.height as usize))
                .map(|_| Constraint::Length(1))
                .collect::<Vec<_>>(),
        )
        .split(inner);

    for (line, &row) in lines.into_iter().zip(rows.iter()) {
        frame.render_widget(Paragraph::new(line), row);
    }
}

fn sidebar_row(pkg: &PackageState, is_selected: bool) -> Line<'static> {
    let (icon, _, color) = build_system_meta(pkg.build_system);
    let pct = pkg
        .progress_pct
        .map(|p| format!("{p:>3.0}%"))
        .unwrap_or_else(|| "  ? ".to_string());
    let name = pad(&truncate(&pkg.name, NAME_W), NAME_W);
    let ver = pad(&truncate(&pkg.version, VER_W), VER_W);
    if is_selected {
        Line::from(vec![
            Span::styled("▎", Style::default().fg(ACCENT).bg(HIGHLIGHT_BG)),
            Span::styled(
                format!(" {icon} "),
                Style::default().fg(color).bg(HIGHLIGHT_BG),
            ),
            Span::styled(
                name,
                Style::default()
                    .fg(TEXT)
                    .add_modifier(Modifier::BOLD)
                    .bg(HIGHLIGHT_BG),
            ),
            Span::styled(ver, Style::default().fg(DIM).bg(HIGHLIGHT_BG)),
            Span::styled(pct, Style::default().fg(ACCENT).bg(HIGHLIGHT_BG)),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(format!(" {icon} "), Style::default().fg(color)),
            Span::styled(name, Style::default().fg(TEXT)),
            Span::styled(ver, Style::default().fg(FAINT)),
            Span::styled(pct, Style::default().fg(FAINT)),
        ])
    }
}

fn sidebar_stat(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(pad(label, 6), Style::default().fg(DIM)),
        Span::styled(value, Style::default().fg(TEXT)),
    ])
}

fn sidebar_gauge(label: &'static str, pct: f32, value: String) -> Line<'static> {
    let (bar, color) = gauge_bar(pct, 10);
    Line::from(vec![
        Span::styled(pad(label, 6), Style::default().fg(DIM)),
        Span::styled(bar, Style::default().fg(color)),
        Span::styled(format!(" {value}"), Style::default().fg(TEXT)),
    ])
}

fn render_history_chart(frame: &mut Frame, area: Rect, history: &VecDeque<f32>) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let rows_total = area.height as usize;
    let cols_total = area.width as usize;
    let samples: Vec<f32> = history.iter().rev().take(cols_total).copied().collect();

    for row in 0..rows_total {
        let level_from_bottom = rows_total - row;
        let band = level_from_bottom as f32 / rows_total as f32;
        let color = if band > 0.66 {
            BLUE
        } else if band > 0.33 {
            ACCENT
        } else {
            GREEN
        };

        let mut spans = Vec::with_capacity(cols_total);
        for x in 0..cols_total {
            let sample_idx = cols_total.saturating_sub(1) - x;
            let value = samples
                .get(sample_idx)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 100.0);
            let filled_rows = ((value / 100.0) * rows_total as f32).round() as usize;
            if filled_rows >= level_from_bottom {
                spans.push(Span::styled("█", Style::default().fg(color)));
            } else {
                spans.push(Span::raw(" "));
            }
        }
        let row_area = Rect {
            x: area.x,
            y: area.y + row as u16,
            width: area.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }
}

pub fn render_view_building(
    frame: &mut Frame,
    area: Rect,
    state: &SystemCompileState,
    selected: usize,
    history: &VecDeque<f32>,
) {
    let block = panel("build");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(pkg) = state.active_packages.get(selected) else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Min(3)])
            .split(inner);
        let msg = vcenter(
            vec![
                centered(
                    "· idle ·",
                    Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                ),
                centered(
                    "waiting for emerge to start a build…",
                    Style::default().fg(FAINT),
                ),
            ],
            rows[0].height,
        );
        frame.render_widget(Paragraph::new(msg), rows[0]);
        render_history_chart(frame, rows[1], history);
        return;
    };

    let (icon, label, color) = build_system_meta(pkg.build_system);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
        ])
        .split(inner);

    let identity = Line::from(vec![
        pill(format!("{icon} {label}"), color),
        Span::raw("  "),
        Span::styled(
            pkg.full_name(),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(pkg.category.clone(), Style::default().fg(DIM)),
    ]);
    frame.render_widget(Paragraph::new(identity), rows[0]);

    let stat = |key: &'static str, value: String| -> Vec<Span<'static>> {
        vec![
            Span::styled(format!("{key} "), Style::default().fg(DIM)),
            Span::styled(
                value,
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]
    };
    let mut stats = Vec::new();
    stats.extend(stat("elapsed", fmt_duration(pkg.elapsed_secs())));
    stats.push(Span::raw("    "));
    let pid_display = match (pkg.pid, pkg.process_count) {
        (Some(p), n) if n > 1 => format!("{p} (+{})", n - 1),
        (Some(p), _) => p.to_string(),
        (None, _) => "—".to_string(),
    };
    stats.extend(stat("pid", pid_display));
    stats.push(Span::raw("    "));
    stats.extend(stat("cpu", format!("{:.0}%", pkg.cpu_usage)));
    stats.push(Span::raw("    "));
    stats.extend(stat("mem", format!("{} MB", pkg.memory_mb)));
    frame.render_widget(Paragraph::new(Line::from(stats)), rows[2]);

    let pct = pkg.progress_pct.unwrap_or(0.0).clamp(0.0, 100.0);
    let bar_width = (rows[4].width as usize).saturating_sub(16);
    let filled = ((pct / 100.0) * bar_width as f32).round() as usize;
    let progress = Line::from(vec![
        Span::styled("progress ", Style::default().fg(DIM)),
        Span::styled(
            "█".repeat(filled.min(bar_width)),
            Style::default().fg(ACCENT),
        ),
        Span::styled(
            "░".repeat(bar_width.saturating_sub(filled)),
            Style::default().fg(FAINT),
        ),
        Span::styled(
            format!(" {pct:>3.0}%"),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(progress), rows[4]);

    let chart_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(FAINT))
        .title(Span::styled(
            " cpu activity · 60s ",
            Style::default().fg(FAINT),
        ));
    let chart_inner = chart_block.inner(rows[6]);
    frame.render_widget(chart_block, rows[6]);
    render_history_chart(frame, chart_inner, history);
}

pub fn render_view_log(frame: &mut Frame, area: Rect, state: &SystemCompileState, selected: usize) {
    let block = panel("log");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(pkg) = state.active_packages.get(selected) else {
        frame.render_widget(
            Paragraph::new(centered(
                "no build selected",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )),
            inner,
        );
        return;
    };

    if pkg.recent_lines.is_empty() {
        frame.render_widget(
            Paragraph::new(centered(
                "waiting for log output…",
                Style::default().fg(FAINT),
            )),
            inner,
        );
        return;
    }

    let mut ordered: Vec<&str> = pkg.recent_lines.iter().map(|s| s.as_str()).collect();
    ordered.reverse();
    let last_idx = ordered.len() - 1;
    let avail = (inner.width as usize).saturating_sub(2);

    let mut lines: Vec<Line> = Vec::with_capacity(ordered.len());
    for (i, text) in ordered.iter().enumerate() {
        let (marker, style) = if i == last_idx {
            (
                Span::styled("▎", Style::default().fg(ACCENT)),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            )
        } else if last_idx - i <= 2 {
            (Span::raw(" "), Style::default().fg(DIM))
        } else {
            (Span::raw(" "), Style::default().fg(FAINT))
        };
        lines.push(Line::from(vec![
            marker,
            Span::raw(" "),
            Span::styled(truncate(text, avail), style),
        ]));
    }

    let padded = vcenter(lines, inner.height);
    frame.render_widget(Paragraph::new(padded), inner);
}

const RES_NAME_W: usize = 22;
const RES_PID_W: usize = 8;
const RES_CPU_W: usize = 6;
const RES_MEM_W: usize = 10;

pub fn render_view_system(frame: &mut Frame, area: Rect, state: &SystemCompileState) {
    let block = panel("resources");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.active_packages.is_empty() {
        frame.render_widget(
            Paragraph::new(centered(
                "nothing to profile right now",
                Style::default().fg(FAINT),
            )),
            inner,
        );
        return;
    }

    let mut pkgs: Vec<&PackageState> = state.active_packages.iter().collect();
    pkgs.sort_by(|a, b| {
        b.cpu_usage
            .partial_cmp(&a.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let fixed = 2 + RES_NAME_W + RES_PID_W + RES_CPU_W + RES_MEM_W;
    let step_w = (inner.width as usize).saturating_sub(fixed);

    let mut lines: Vec<Line> = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled(pad("package", RES_NAME_W), Style::default().fg(DIM)),
        Span::styled(pad("pid", RES_PID_W), Style::default().fg(DIM)),
        Span::styled(pad("cpu", RES_CPU_W), Style::default().fg(DIM)),
        Span::styled(pad("mem", RES_MEM_W), Style::default().fg(DIM)),
        Span::styled("last step", Style::default().fg(DIM)),
    ])];
    lines.push(Line::from(Span::styled(
        "╌".repeat(inner.width as usize),
        Style::default().fg(FAINT),
    )));

    for pkg in &pkgs {
        let (icon, _, color) = build_system_meta(pkg.build_system);
        let pid = match (pkg.pid, pkg.process_count) {
            (Some(p), n) if n > 1 => format!("{p}+{}", n - 1),
            (Some(p), _) => p.to_string(),
            (None, _) => "—".to_string(),
        };
        let step = pkg.current_step.clone().unwrap_or_else(|| "…".to_string());
        lines.push(Line::from(vec![
            Span::styled(format!("{icon} "), Style::default().fg(color)),
            Span::styled(
                pad(&truncate(&pkg.full_name(), RES_NAME_W), RES_NAME_W),
                Style::default().fg(TEXT),
            ),
            Span::styled(pad(&pid, RES_PID_W), Style::default().fg(DIM)),
            Span::styled(
                pad(&format!("{:.0}%", pkg.cpu_usage), RES_CPU_W),
                Style::default().fg(gauge_color(pkg.cpu_usage)),
            ),
            Span::styled(
                pad(&format!("{} MB", pkg.memory_mb), RES_MEM_W),
                Style::default().fg(TEXT),
            ),
            Span::styled(truncate(&step, step_w), Style::default().fg(DIM)),
        ]));
    }

    let content = if lines.len() * 3 < inner.height as usize {
        vcenter(lines, inner.height)
    } else {
        lines
    };
    frame.render_widget(Paragraph::new(content), inner);
}

pub fn render_footer(
    frame: &mut Frame,
    area: Rect,
    state: &SystemCompileState,
    selected: usize,
    history: &VecDeque<f32>,
    view: View,
) {
    let _ = (view, history);
    let block = panel("status");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let pkg = state.active_packages.get(selected);
    let status_line = match pkg {
        Some(p) => {
            let (icon, label, color) = build_system_meta(p.build_system);
            Line::from(vec![
                Span::styled("● ", Style::default().fg(ACCENT)),
                Span::styled(
                    p.full_name(),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                pill(format!("{icon} {label}"), color),
                Span::raw("   "),
                Span::styled(
                    format!(
                        "pid {}",
                        p.pid
                            .map(|x| x.to_string())
                            .unwrap_or_else(|| "—".to_string())
                    ),
                    Style::default().fg(DIM),
                ),
                Span::raw("   "),
                Span::styled(fmt_duration(p.elapsed_secs()), Style::default().fg(DIM)),
                Span::raw("   "),
                Span::styled(format!("cpu {:.0}%", p.cpu_usage), Style::default().fg(DIM)),
                Span::raw("   "),
                Span::styled(format!("mem {} MB", p.memory_mb), Style::default().fg(DIM)),
            ])
        }
        None => Line::from(Span::styled("nothing selected", Style::default().fg(FAINT))),
    };
    frame.render_widget(Paragraph::new(status_line), rows[0]);

    let mut spans = Vec::new();
    for (i, (k, d)) in [
        ("↑↓", "select"),
        ("↔", "view"),
        ("r", "refresh"),
        ("q", "quit"),
    ]
    .iter()
    .enumerate()
    {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            *k,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {d}"), Style::default().fg(DIM)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
}
