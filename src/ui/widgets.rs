use crate::View;
use crate::model::{BuildSystem, PackageState, SystemCompileState};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table, TableState,
};
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

fn joined(groups: Vec<Vec<Span<'static>>>, sep: Span<'static>) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, group) in groups.into_iter().enumerate() {
        if i > 0 {
            spans.push(sep.clone());
        }
        spans.extend(group);
    }
    Line::from(spans)
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
    const LOGO_TEXT: &str = " gentooplz ";

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(LOGO_TEXT.len() as u16),
            Constraint::Min(10),
        ])
        .split(area);

    let logo = Line::from(vec![Span::styled(
        LOGO_TEXT,
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

pub fn render_sidebar(
    frame: &mut Frame,
    area: Rect,
    state: &SystemCompileState,
    selected: usize,
    table_state: &mut TableState,
) {
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

    let stats_height = 6u16.min(inner.height);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(stats_height)])
        .split(inner);
    let (table_area, stats_area) = (sections[0], sections[1]);

    if n == 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "nothing compiling",
                Style::default().fg(FAINT).add_modifier(Modifier::ITALIC),
            )),
            table_area,
        );
    } else {
        let rows: Vec<Row> = state
            .active_packages
            .iter()
            .map(|pkg| {
                let (icon, _, color) = build_system_meta(pkg.build_system);
                let pct = pkg
                    .progress_pct
                    .map(|p| format!("{p:>3.0}%"))
                    .unwrap_or_else(|| "  ? ".to_string());
                Row::new(vec![
                    Cell::from(icon.to_string()).style(Style::default().fg(color)),
                    Cell::from(truncate(&pkg.name, NAME_W)).style(Style::default().fg(TEXT)),
                    Cell::from(truncate(&pkg.version, VER_W)).style(Style::default().fg(DIM)),
                    Cell::from(pct).style(Style::default().fg(DIM)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(3),
            Constraint::Length(NAME_W as u16),
            Constraint::Length(VER_W as u16),
            Constraint::Length(5),
        ];

        let table = Table::new(rows, widths)
            .column_spacing(1)
            .row_highlight_style(
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▎");

        table_state.select(Some(selected));
        frame.render_stateful_widget(table, table_area, table_state);
    }

    let mem_pct = if state.system_mem_total_mb > 0 {
        (state.system_mem_used_mb as f32 / state.system_mem_total_mb as f32) * 100.0
    } else {
        0.0
    };

    let stat_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1); 6])
        .split(stats_area);

    frame.render_widget(
        Paragraph::new(Span::styled(
            "╌".repeat(stats_area.width as usize),
            Style::default().fg(FAINT),
        )),
        stat_rows[1],
    );
    render_sidebar_gauge(
        frame,
        stat_rows[3],
        "cpu",
        state.system_cpu_pct,
        format!("{:>2.0}%", state.system_cpu_pct.min(99.9)),
    );
    render_sidebar_gauge(
        frame,
        stat_rows[4],
        "ram",
        mem_pct,
        format!("{:>2.0}%", mem_pct.min(99.0)),
    );
    frame.render_widget(
        Paragraph::new(sidebar_stat("load", format!("{:.2}", state.load_avg_1))),
        stat_rows[5],
    );
}

fn sidebar_stat(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(pad(label, 6), Style::default().fg(DIM)),
        Span::styled(value, Style::default().fg(TEXT)),
    ])
}

fn render_sidebar_gauge(
    frame: &mut Frame,
    area: Rect,
    label: &'static str,
    pct: f32,
    value: String,
) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(4),
            Constraint::Length(value.chars().count() as u16 + 1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Span::styled(pad(label, 6), Style::default().fg(DIM))),
        cols[0],
    );

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color(pct)).bg(FAINT))
        .ratio((pct.clamp(0.0, 100.0) / 100.0) as f64)
        .label("");
    frame.render_widget(gauge, cols[1]);

    frame.render_widget(
        Paragraph::new(Span::styled(format!(" {value}"), Style::default().fg(TEXT))),
        cols[2],
    );
}

fn render_history_chart(frame: &mut Frame, area: Rect, history: &VecDeque<f32>) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let data: Vec<u64> = history
        .iter()
        .map(|v| v.clamp(0.0, 100.0).round() as u64)
        .collect();

    let sparkline = Sparkline::default()
        .data(&data)
        .max(100)
        .style(Style::default().fg(ACCENT));
    frame.render_widget(sparkline, area);
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

    let identity = joined(
        vec![
            vec![pill(format!("{icon} {label}"), color)],
            vec![Span::styled(
                pkg.full_name(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            )],
            vec![Span::styled(pkg.category.clone(), Style::default().fg(DIM))],
        ],
        Span::raw("  "),
    );
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
    let pid_display = match (pkg.pid, pkg.process_count) {
        (Some(p), n) if n > 1 => format!("{p} (+{})", n - 1),
        (Some(p), _) => p.to_string(),
        (None, _) => "—".to_string(),
    };
    let stats = joined(
        vec![
            stat("elapsed", fmt_duration(pkg.elapsed_secs())),
            stat("pid", pid_display),
            stat("cpu", format!("{:.0}%", pkg.cpu_usage)),
            stat("mem", format!("{} MB", pkg.memory_mb)),
        ],
        Span::raw("    "),
    );
    frame.render_widget(Paragraph::new(stats), rows[2]);

    let pct = pkg.progress_pct.unwrap_or(0.0).clamp(0.0, 100.0);
    let progress_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(9), Constraint::Min(10)])
        .split(rows[4]);
    frame.render_widget(
        Paragraph::new(Span::styled("progress ", Style::default().fg(DIM))),
        progress_cols[0],
    );
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(ACCENT).bg(FAINT))
        .ratio((pct / 100.0) as f64)
        .label(format!("{pct:>3.0}%"));
    frame.render_widget(gauge, progress_cols[1]);

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

    let header = Row::new(vec!["", "package", "pid", "cpu", "mem", "last step"])
        .style(Style::default().fg(DIM));

    let rows: Vec<Row> = pkgs
        .iter()
        .map(|pkg| {
            let (icon, _, color) = build_system_meta(pkg.build_system);
            let pid = match (pkg.pid, pkg.process_count) {
                (Some(p), n) if n > 1 => format!("{p}+{}", n - 1),
                (Some(p), _) => p.to_string(),
                (None, _) => "—".to_string(),
            };
            let step = pkg.current_step.clone().unwrap_or_else(|| "…".to_string());
            Row::new(vec![
                Cell::from(icon.to_string()).style(Style::default().fg(color)),
                Cell::from(truncate(&pkg.full_name(), RES_NAME_W)).style(Style::default().fg(TEXT)),
                Cell::from(pid).style(Style::default().fg(DIM)),
                Cell::from(format!("{:.0}%", pkg.cpu_usage))
                    .style(Style::default().fg(gauge_color(pkg.cpu_usage))),
                Cell::from(format!("{} MB", pkg.memory_mb)).style(Style::default().fg(TEXT)),
                Cell::from(step).style(Style::default().fg(DIM)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Length(RES_NAME_W as u16),
        Constraint::Length(RES_PID_W as u16),
        Constraint::Length(RES_CPU_W as u16),
        Constraint::Length(RES_MEM_W as u16),
        Constraint::Min(10),
    ];

    let table = Table::new(rows, widths).header(header).column_spacing(1);

    frame.render_widget(table, inner);
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
            let pid = p
                .pid
                .map(|x| x.to_string())
                .unwrap_or_else(|| "—".to_string());
            joined(
                vec![
                    vec![
                        Span::styled("● ", Style::default().fg(ACCENT)),
                        Span::styled(
                            p.full_name(),
                            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                        ),
                    ],
                    vec![pill(format!("{icon} {label}"), color)],
                    vec![Span::styled(format!("pid {pid}"), Style::default().fg(DIM))],
                    vec![Span::styled(
                        fmt_duration(p.elapsed_secs()),
                        Style::default().fg(DIM),
                    )],
                    vec![Span::styled(
                        format!("cpu {:.0}%", p.cpu_usage),
                        Style::default().fg(DIM),
                    )],
                    vec![Span::styled(
                        format!("mem {} MB", p.memory_mb),
                        Style::default().fg(DIM),
                    )],
                ],
                Span::raw("   "),
            )
        }
        None => Line::from(Span::styled("nothing selected", Style::default().fg(FAINT))),
    };
    frame.render_widget(Paragraph::new(status_line), rows[0]);

    let keybinds = joined(
        [
            ("↑↓", "select"),
            ("↔", "view"),
            ("r", "refresh"),
            ("q", "quit"),
        ]
        .into_iter()
        .map(|(k, d)| {
            vec![
                Span::styled(k, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {d}"), Style::default().fg(DIM)),
            ]
        })
        .collect(),
        Span::raw("  "),
    );
    frame.render_widget(Paragraph::new(keybinds), rows[1]);
}
