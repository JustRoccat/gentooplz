mod model;
mod parser;
mod scanner;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use model::{ScanEvent, SystemCompileState};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::collections::VecDeque;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, default_value_t = 1000)]
    interval: u64,

    #[arg(short, long, default_value = "/var/tmp/portage")]
    portage_tmp: PathBuf,

    #[arg(short, long, default_value = "/var/log/emerge.log")]
    emerge_log: PathBuf,
}

const HISTORY_LEN: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Building,
    Log,
    System,
}

impl View {
    pub const ALL: [View; 3] = [View::Building, View::Log, View::System];

    pub fn label(self) -> &'static str {
        match self {
            View::Building => "Build",
            View::Log => "Log",
            View::System => "Resources",
        }
    }

    fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

struct App {
    state: SystemCompileState,
    should_quit: bool,
    selected: usize,
    cpu_history: VecDeque<f32>,
    view: View,
}

impl App {
    fn new() -> Self {
        Self {
            state: SystemCompileState::default(),
            should_quit: false,
            selected: 0,
            cpu_history: VecDeque::with_capacity(HISTORY_LEN),
            view: View::Building,
        }
    }

    fn apply_update(&mut self, state: SystemCompileState) {
        self.cpu_history.push_back(state.system_cpu_pct);
        while self.cpu_history.len() > HISTORY_LEN {
            self.cpu_history.pop_front();
        }

        let len = state.active_packages.len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }

        self.state = state;
    }

    fn handle_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.state.active_packages.len();
                if len > 0 && self.selected + 1 < len {
                    self.selected += 1;
                }
            }
            KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => {
                self.view = self.view.next();
            }
            KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => {
                self.view = self.view.prev();
            }
            _ => {}
        }
    }
}

fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn is_on_path(bin: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| dir.join(bin).is_file())
}

fn invoked_command_line() -> String {
    let mut args = std::env::args();
    let exe = args.next().unwrap_or_else(|| "gentooplz".to_string());

    let mut out = exe;
    for arg in args {
        out.push(' ');
        if arg.chars().any(char::is_whitespace) {
            out.push('\'');
            out.push_str(&arg);
            out.push('\'');
        } else {
            out.push_str(&arg);
        }
    }
    out
}

fn print_root_required_message() {
    use crossterm::style::Stylize;

    let has_doas = is_on_path("doas");
    let has_sudo = is_on_path("sudo");
    let cmd = invoked_command_line();

    let title = "  Root Needed  ";
    let bar = "-".repeat(title.chars().count());

    println!();
    println!("{}", format!("┌{bar}┐").dark_yellow());
    println!("{}", format!("│{title}│").dark_yellow().bold());
    println!("{}", format!("└{bar}┘").dark_yellow());
    println!();
    println!(
        "  {} reads Portage working directories (np. {}) and",
        "gentooplz".bold(),
        "/var/tmp/portage".cyan()
    );
    println!(
        "  list of processes of other users in {} - because of that",
        "/proc".cyan()
    );
    println!("  must be run as {}.", "root".bold().red());
    println!();

    let suggest = |tool: &str, cmd: &str| {
        println!("  Detected {} run it:", tool.bold());
        println!();
        println!("      {} {}", tool.green().bold(), cmd.green().bold());
        println!();
    };

    match (has_doas, has_sudo) {
        (true, false) => suggest("doas", &cmd),
        (false, true) => suggest("sudo", &cmd),
        (true, true) => {
            println!(
                "Found both (lol) {}, and {} - use the one",
                "doas".bold(),
                "sudo".bold()
            );
            println!("  that you normally would use:");
            println!();
            println!(
                "      {} {}",
                "doas".green().bold(),
                cmd.as_str().green().bold()
            );
            println!(
                "      {} {}",
                "sudo".green().bold(),
                cmd.as_str().green().bold()
            );
            println!();
        }
        (false, false) => {
            println!(
                "  Not found {}, or {} in $PATH.",
                "doas".bold(),
                "sudo".bold()
            );
            println!("  Log in as root (with {}) and run:", "su".bold());
            println!();
            println!("      {}", cmd.as_str().green().bold());
            println!();
        }
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if !running_as_root() {
        print_root_required_message();
        std::process::exit(1);
    }

    let (tx, mut rx) = mpsc::channel::<ScanEvent>(16);

    let scanner_handle = tokio::spawn(scanner::run_scanner_loop(
        tx,
        cli.interval,
        cli.portage_tmp,
        cli.emerge_log,
    ));

    let mut terminal = setup_terminal()?;
    let mut app = App::new();

    let result = run_app(&mut terminal, &mut app, &mut rx).await;

    restore_terminal(&mut terminal)?;
    scanner_handle.abort();

    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    rx: &mut mpsc::Receiver<ScanEvent>,
) -> Result<()> {
    loop {
        while let Ok(event) = rx.try_recv() {
            match event {
                ScanEvent::Update(state) => app.apply_update(state),
                ScanEvent::Error(_msg) => {}
            }
        }

        terminal
            .draw(|frame| ui::draw(frame, &app.state, app.selected, &app.cpu_history, app.view))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
