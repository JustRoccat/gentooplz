use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildSystem {
    Ninja,
    Make,
    Cargo,
    #[default]
    Unknown,
}

#[derive(Debug, Clone)]
pub struct PackageState {
    pub category: String,
    pub name: String,
    pub version: String,
    pub build_dir: PathBuf,
    pub build_system: BuildSystem,
    pub current_step: Option<String>,
    pub recent_lines: Vec<String>,
    pub progress_pct: Option<f32>,
    pub memory_mb: u64,
    pub cpu_usage: f32,
    pub started_at: Instant,
    pub pid: Option<u32>,
    pub process_count: usize,
}

impl PackageState {
    pub fn full_name(&self) -> String {
        format!("{}/{}-{}", self.category, self.name, self.version)
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SystemCompileState {
    pub current_job: usize,
    pub total_jobs: usize,
    pub active_packages: Vec<PackageState>,
    pub system_cpu_pct: f32,
    pub system_mem_used_mb: u64,
    pub system_mem_total_mb: u64,
    pub load_avg_1: f64,
}

#[derive(Debug, Clone)]
pub enum ScanEvent {
    Update(SystemCompileState),
    Error(String),
}
