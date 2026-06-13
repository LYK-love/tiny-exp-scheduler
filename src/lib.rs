use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const DEFAULT_SCHEDULER_WINDOW_NAME: &str = "__sched__";
const DEFAULT_JOB_WINDOW_PREFIX: &str = "job";
const AUTO_SCHEDULER_WINDOW_PREFIX: &str = "__sched_";
const AUTO_SCHEDULER_WINDOW_SUFFIX: &str = "__";

struct TmuxClient;

impl TmuxClient {
    fn ensure_available(&self) -> io::Result<()> {
        let status = Command::new("tmux")
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "tmux is required but not available",
            ))
        }
    }

    fn current_session(&self) -> io::Result<String> {
        let output = Command::new("tmux")
            .args(["display-message", "-p", "#S"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;
        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "failed to resolve current tmux session",
            ));
        }
        let session = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if session.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "current tmux session name is empty",
            ));
        }
        Ok(session)
    }

    fn current_window_name(&self) -> io::Result<String> {
        let output = Command::new("tmux")
            .args(["display-message", "-p", "#{window_name}"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;
        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "failed to resolve current tmux window name",
            ));
        }
        let window_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if window_name.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "current tmux window name is empty",
            ));
        }
        Ok(window_name)
    }

    fn rename_current_window(&self, name: &str) -> io::Result<()> {
        let status = Command::new("tmux")
            .args(["rename-window", name])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("failed to rename current tmux window to {name}"),
            ))
        }
    }

    fn window_names(&self, session: &str) -> io::Result<Vec<String>> {
        let output = Command::new("tmux")
            .args(["list-windows", "-t", session, "-F", "#{window_name}"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;
        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("failed to list tmux windows for session {session}"),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    fn has_window(&self, session: &str, window_name: &str) -> io::Result<bool> {
        Ok(self
            .window_names(session)?
            .into_iter()
            .any(|name| name == window_name))
    }

    fn start_job_window(
        &self,
        session: &str,
        window_name: &str,
        script: &str,
        keep_on_exit: bool,
    ) -> io::Result<()> {
        let shell_command = build_tmux_shell_command(script);
        let status = Command::new("tmux")
            .args([
                "new-window",
                "-d",
                "-t",
                session,
                "-n",
                window_name,
                &shell_command,
            ])
            .status()?;
        if !status.success() {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("failed to start tmux window: {window_name}"),
            ))
        } else if keep_on_exit {
            self.set_remain_on_exit(session, window_name, true)
        } else {
            Ok(())
        }
    }

    fn set_remain_on_exit(
        &self,
        session: &str,
        window_name: &str,
        enabled: bool,
    ) -> io::Result<()> {
        let value = if enabled { "on" } else { "off" };
        let target = format!("{session}:{window_name}");
        let status = Command::new("tmux")
            .args(["set-option", "-t", &target, "remain-on-exit", value])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("failed to set remain-on-exit for {window_name}"),
            ))
        }
    }

    fn pane_dead(&self, session: &str, window_name: &str) -> io::Result<bool> {
        let target = format!("{session}:{window_name}");
        let output = Command::new("tmux")
            .args(["list-panes", "-t", &target, "-F", "#{pane_dead}"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;
        if !output.status.success() {
            return Ok(false);
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .map(|line| line.trim() == "1")
            .unwrap_or(false))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Scheduled,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: usize,
    pub cmd: String,
    pub status: JobStatus,
    pub gpu_id: Option<usize>,
    pub cpu_cores: Option<Vec<usize>>,
    pub window_name: Option<String>,
    pub log_path: PathBuf,
    pub exit_path: PathBuf,
}

impl Job {
    pub fn new(id: usize, cmd: String, logs_dir: &Path) -> Self {
        let base = format!("job_{id}");
        Self {
            id,
            cmd,
            status: JobStatus::Pending,
            gpu_id: None,
            cpu_cores: None,
            window_name: None,
            log_path: logs_dir.join(format!("{base}.log")),
            exit_path: logs_dir.join(format!("{base}.exit")),
        }
    }

    pub fn name(&self) -> String {
        format!("job_{}", self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub input_path: Option<PathBuf>,
    pub logs_dir: PathBuf,
    pub scheduler_name: Option<String>,
    pub cpu_threads: Option<usize>,
    pub cpu_cores: CpuCoresArg,
    pub cpus_per_job: Option<usize>,
    pub cuda_devices: CudaDevicesArg,
    pub idle_memory_threshold_mb: usize,
    pub idle_utilization_threshold: usize,
    pub tick_seconds: u64,
    pub dry_run: bool,
    pub keep_job_tabs: bool,
    pub verbose: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            input_path: None,
            logs_dir: PathBuf::from("logs"),
            scheduler_name: None,
            cpu_threads: None,
            cpu_cores: CpuCoresArg::None,
            cpus_per_job: None,
            cuda_devices: CudaDevicesArg::Auto,
            idle_memory_threshold_mb: 64,
            idle_utilization_threshold: 0,
            tick_seconds: 1,
            dry_run: false,
            keep_job_tabs: false,
            verbose: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CudaDevicesArg {
    Auto,
    None,
    Explicit(Vec<usize>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuCoresArg {
    None,
    Auto,
    Explicit(Vec<usize>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunNamespace {
    scheduler_window_name: String,
    job_window_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GpuSnapshot {
    index: usize,
    memory_used_mb: usize,
    utilization_gpu: usize,
}

pub fn run_cli<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let argv: Vec<String> = args.into_iter().map(Into::into).collect();
    match parse_args(&argv)? {
        CliAction::Help => {
            println!("{}", help_text());
            Ok(())
        }
        CliAction::Run(options) => run(options).map_err(|err| err.to_string()),
    }
}

#[derive(Debug)]
enum CliAction {
    Help,
    Run(RunOptions),
}

fn parse_args(argv: &[String]) -> Result<CliAction, String> {
    if argv.len() <= 1 {
        return Ok(CliAction::Help);
    }

    match argv[1].as_str() {
        "-h" | "--help" => Ok(CliAction::Help),
        "run" => parse_run_args(argv),
        other => Err(format!("unknown command: {other}\n\n{}", help_text())),
    }
}

fn parse_run_args(argv: &[String]) -> Result<CliAction, String> {
    let mut options = RunOptions::default();
    let mut i = 2;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => return Ok(CliAction::Help),
            "--logs-dir" => {
                i += 1;
                let value = argv.get(i).ok_or("--logs-dir requires a value")?;
                options.logs_dir = PathBuf::from(value);
            }
            "--scheduler-name" => {
                i += 1;
                let value = argv.get(i).ok_or("--scheduler-name requires a value")?;
                options.scheduler_name = Some(parse_scheduler_name_arg(value)?);
            }
            "--cpu-threads" => {
                i += 1;
                let value = argv.get(i).ok_or("--cpu-threads requires a value")?;
                options.cpu_threads = Some(parse_positive_usize_arg("--cpu-threads", value)?);
            }
            "--cpu-cores" => {
                i += 1;
                let value = argv.get(i).ok_or("--cpu-cores requires a value")?;
                options.cpu_cores = parse_cpu_cores_arg(value)?;
            }
            "--cpus-per-job" => {
                i += 1;
                let value = argv.get(i).ok_or("--cpus-per-job requires a value")?;
                options.cpus_per_job = Some(parse_positive_usize_arg("--cpus-per-job", value)?);
            }
            "--cuda-devices" => {
                i += 1;
                let value = argv.get(i).ok_or("--cuda-devices requires a value")?;
                options.cuda_devices = parse_cuda_devices_arg(value)?;
            }
            "--idle-memory-threshold-mb" => {
                i += 1;
                let value = argv
                    .get(i)
                    .ok_or("--idle-memory-threshold-mb requires a value")?;
                options.idle_memory_threshold_mb = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --idle-memory-threshold-mb value: {value}"))?;
            }
            "--idle-utilization-threshold" => {
                i += 1;
                let value = argv
                    .get(i)
                    .ok_or("--idle-utilization-threshold requires a value")?;
                options.idle_utilization_threshold = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --idle-utilization-threshold value: {value}"))?;
            }
            "--tick-seconds" => {
                i += 1;
                let value = argv.get(i).ok_or("--tick-seconds requires a value")?;
                options.tick_seconds = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --tick-seconds value: {value}"))?;
            }
            "--dry-run" => {
                options.dry_run = true;
            }
            "--keep-job-tabs" => {
                options.keep_job_tabs = true;
            }
            "--verbose" => {
                options.verbose = true;
            }
            arg if arg.starts_with("--") => {
                return Err(format!("unknown option: {arg}\n\n{}", help_text()));
            }
            path => {
                if options.input_path.is_some() {
                    return Err("only one input file is allowed".to_string());
                }
                options.input_path = Some(PathBuf::from(path));
            }
        }
        i += 1;
    }

    Ok(CliAction::Run(options))
}

pub fn help_text() -> String {
    let text = r#"tiny-exp-scheduler

A minimal tmux-based scheduler for explicit shell commands on selected CUDA devices.

USAGE:
  tiny-exp-scheduler run [COMMANDS_FILE] [OPTIONS]
  cat commands.txt | tiny-exp-scheduler run [OPTIONS]
  tiny-exp-scheduler -h | --help

MODEL:
  Run inside an existing tmux session.
  The current tab becomes an available scheduler tab, such as __sched__.
  Job tabs are appended after it.
  The scheduler tab stays open for the final summary.

OPTIONS:
  --logs-dir DIR      Directory for log and exit-code files. Default: logs
  --scheduler-name NAME
                      Stable tmux namespace for this run. Window: __sched_NAME__.
                      Jobs: NAME_job_1, NAME_job_2, ...
  --cpu-threads N     Limit CPU compute threads per job by setting common
                      OpenMP/BLAS/PyTorch environment variables.
  --cpu-cores ARG     CPU core pool for affinity allocation: 'auto', 'none',
                      or a comma-separated list/range like 0-15,32-47.
                      Default: none
  --cpus-per-job N    Number of CPU cores to allocate per running job.
                      Required for --cpu-cores auto.
  --cuda-devices ARG  'auto', 'none', or a comma-separated list like 0,2,5.
                      Default: auto
  --idle-memory-threshold-mb N
                      Max memory.used for an idle GPU. Default: 64
  --idle-utilization-threshold N
                      Max utilization.gpu for an idle GPU. Default: 0
  --tick-seconds N    Scheduler polling interval in seconds. Default: 1
  --dry-run           Resolve GPUs and print the plan without touching tmux.
  --keep-job-tabs     Keep finished job tabs open in tmux. Default: off
  --verbose           Print per-job runtime events in the scheduler tab.
  -h, --help          Show this help message.

CUDA DEVICES:
  auto                Detect idle GPUs once at startup and freeze that set.
  none                Do not allocate GPUs or set CUDA_VISIBLE_DEVICES.
  0,2,5               Use exactly these GPUs; all must already be idle.
  idle rule           memory.used <= memory threshold and utilization.gpu <= utilization threshold
  startup output      Prints the final CUDA device range.

CPU CORES:
  none                Do not allocate CPU affinity slots.
  auto                Use all logical CPU cores, split by --cpus-per-job.
  0-15,32-47          Use exactly these logical CPU cores.
  --cpus-per-job N    Split the CPU pool into fixed-size slots.
  thread limit        If --cpu-threads is omitted, each CPU slot uses N threads.

INPUT RULES:
  - ignore empty lines
  - ignore lines starting with '#'
  - each remaining line is one job

RUNTIME:
  By default, one running job occupies one GPU.
  By default, finished job tabs exit and disappear.
  With --keep-job-tabs, finished job tabs stay visible in tmux.
"#;
    text.to_string()
}

pub fn run(options: RunOptions) -> io::Result<()> {
    let tmux = TmuxClient;
    let commands = read_commands(options.input_path.as_deref())?;
    if commands.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no jobs found in input",
        ));
    }

    let gpu_devices = resolve_cuda_devices(
        &options.cuda_devices,
        options.idle_memory_threshold_mb,
        options.idle_utilization_threshold,
    )?;
    let cpu_slots = resolve_cpu_slots(&options.cpu_cores, options.cpus_per_job)?;
    if !matches!(options.cuda_devices, CudaDevicesArg::None) && gpu_devices.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no usable CUDA devices resolved",
        ));
    }

    if options.dry_run {
        println!(
            "{}",
            build_dry_run_summary(&options, &gpu_devices, &cpu_slots, commands.len())
        );
        return Ok(());
    }

    tmux.ensure_available()?;
    if !cpu_slots.is_empty() {
        ensure_taskset_available()?;
    }
    ensure_running_inside_tmux()?;
    fs::create_dir_all(&options.logs_dir)?;

    let session = tmux.current_session()?;
    let current_window = tmux.current_window_name()?;
    let namespace = resolve_run_namespace(
        &tmux,
        &session,
        &current_window,
        options.scheduler_name.as_deref(),
        commands.len(),
    )?;
    tmux.rename_current_window(&namespace.scheduler_window_name)?;

    println!("Session: {session}");
    println!("Scheduler tab: {}", namespace.scheduler_window_name);
    println!(
        "Current tab renamed to {}.",
        namespace.scheduler_window_name
    );
    println!("Job tabs will be appended after it.");
    println!(
        "Final CUDA device range: {}",
        format_cuda_devices(&gpu_devices)
    );
    println!(
        "CPU threads per job: {}",
        format_cpu_threads(effective_cpu_threads(
            options.cpu_threads,
            options.cpus_per_job
        ))
    );
    println!("CPU slots: {}", format_cpu_slots(&cpu_slots));
    println!("Logs dir: {}", options.logs_dir.display());
    println!("{} jobs in total.", commands.len());
    let mut scheduler = Scheduler::new(
        session,
        options.logs_dir,
        gpu_devices,
        cpu_slots,
        commands,
        namespace.job_window_prefix,
        options.cpu_threads,
        options.keep_job_tabs,
        options.verbose,
    );
    scheduler.run_loop(Duration::from_secs(options.tick_seconds));
    println!();
    println!("{}", scheduler.summary());
    println!("Scheduler is idle; this tab is being kept open.");
    Ok(())
}

pub fn read_commands(input_path: Option<&Path>) -> io::Result<Vec<String>> {
    let mut content = String::new();
    match input_path {
        Some(path) => content = fs::read_to_string(path)?,
        None => {
            io::stdin().read_to_string(&mut content)?;
        }
    }
    Ok(parse_commands(&content))
}

pub fn parse_commands(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect()
}

fn ensure_running_inside_tmux() -> io::Result<()> {
    if std::env::var_os("TMUX").is_some() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "tiny-exp-scheduler must be run inside an existing tmux session",
        ))
    }
}

fn ensure_taskset_available() -> io::Result<()> {
    let status = Command::new("taskset")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "taskset is required when --cpu-cores is enabled",
        )),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "taskset is required when --cpu-cores is enabled",
        )),
        Err(err) => Err(err),
    }
}

fn parse_cuda_devices_arg(value: &str) -> Result<CudaDevicesArg, String> {
    if value == "auto" {
        return Ok(CudaDevicesArg::Auto);
    }
    if value == "none" {
        return Ok(CudaDevicesArg::None);
    }

    let mut devices = Vec::new();
    for part in value.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return Err(format!("invalid --cuda-devices value: {value}"));
        }
        let id = trimmed
            .parse::<usize>()
            .map_err(|_| format!("invalid CUDA device id in --cuda-devices: {trimmed}"))?;
        devices.push(id);
    }

    if devices.is_empty() {
        return Err("--cuda-devices cannot be empty".to_string());
    }

    devices.sort_unstable();
    devices.dedup();
    Ok(CudaDevicesArg::Explicit(devices))
}

fn parse_cpu_cores_arg(value: &str) -> Result<CpuCoresArg, String> {
    if value == "none" {
        return Ok(CpuCoresArg::None);
    }
    if value == "auto" {
        return Ok(CpuCoresArg::Auto);
    }

    let cores = parse_cpu_core_list(value)?;
    Ok(CpuCoresArg::Explicit(cores))
}

fn parse_cpu_core_list(value: &str) -> Result<Vec<usize>, String> {
    let mut cores = Vec::new();
    for part in value.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return Err(format!("invalid --cpu-cores value: {value}"));
        }
        if let Some((start, end)) = trimmed.split_once('-') {
            let start = start
                .parse::<usize>()
                .map_err(|_| format!("invalid CPU core range in --cpu-cores: {trimmed}"))?;
            let end = end
                .parse::<usize>()
                .map_err(|_| format!("invalid CPU core range in --cpu-cores: {trimmed}"))?;
            if start > end {
                return Err(format!("invalid descending CPU core range: {trimmed}"));
            }
            cores.extend(start..=end);
        } else {
            let id = trimmed
                .parse::<usize>()
                .map_err(|_| format!("invalid CPU core id in --cpu-cores: {trimmed}"))?;
            cores.push(id);
        }
    }
    if cores.is_empty() {
        return Err("--cpu-cores cannot be empty".to_string());
    }
    cores.sort_unstable();
    cores.dedup();
    Ok(cores)
}

fn parse_scheduler_name_arg(value: &str) -> Result<String, String> {
    if value.is_empty() {
        return Err("--scheduler-name cannot be empty".to_string());
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(value.to_string())
    } else {
        Err(format!(
            "invalid --scheduler-name value: {value}; use only ASCII letters, digits, '_', '-', or '.'"
        ))
    }
}

fn parse_positive_usize_arg(option: &str, value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid {option} value: {value}"))?;
    if parsed == 0 {
        Err(format!("{option} must be positive"))
    } else {
        Ok(parsed)
    }
}

fn resolve_cpu_slots(
    selection: &CpuCoresArg,
    cpus_per_job: Option<usize>,
) -> io::Result<Vec<Vec<usize>>> {
    match selection {
        CpuCoresArg::None => {
            if cpus_per_job.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--cpus-per-job requires --cpu-cores auto or an explicit core list",
                ));
            }
            Ok(Vec::new())
        }
        CpuCoresArg::Auto => {
            let cpus_per_job = cpus_per_job.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--cpu-cores auto requires --cpus-per-job",
                )
            })?;
            let total = std::thread::available_parallelism()
                .map(|count| count.get())
                .unwrap_or(1);
            split_cpu_pool_into_slots((0..total).collect(), cpus_per_job)
        }
        CpuCoresArg::Explicit(cores) => {
            let cpus_per_job = cpus_per_job.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--cpu-cores with an explicit core list requires --cpus-per-job",
                )
            })?;
            split_cpu_pool_into_slots(cores.clone(), cpus_per_job)
        }
    }
}

fn split_cpu_pool_into_slots(
    cores: Vec<usize>,
    cpus_per_job: usize,
) -> io::Result<Vec<Vec<usize>>> {
    if cpus_per_job == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--cpus-per-job must be positive",
        ));
    }
    if cpus_per_job > cores.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "--cpus-per-job ({cpus_per_job}) exceeds CPU core pool size ({})",
                cores.len()
            ),
        ));
    }
    let slots = cores
        .chunks(cpus_per_job)
        .filter(|chunk| chunk.len() == cpus_per_job)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();
    if slots.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "CPU core pool produced no usable slots",
        ))
    } else {
        Ok(slots)
    }
}

pub fn resolve_cuda_devices(
    selection: &CudaDevicesArg,
    idle_memory_threshold_mb: usize,
    idle_utilization_threshold: usize,
) -> io::Result<Vec<usize>> {
    match selection {
        CudaDevicesArg::Auto => {
            resolve_auto_cuda_devices(idle_memory_threshold_mb, idle_utilization_threshold)
        }
        CudaDevicesArg::None => Ok(Vec::new()),
        CudaDevicesArg::Explicit(devices) => validate_explicit_cuda_devices(
            devices,
            idle_memory_threshold_mb,
            idle_utilization_threshold,
        ),
    }
}

fn resolve_auto_cuda_devices(
    idle_memory_threshold_mb: usize,
    idle_utilization_threshold: usize,
) -> io::Result<Vec<usize>> {
    let snapshots = query_gpu_snapshots()?;
    let devices: Vec<usize> = snapshots
        .into_iter()
        .filter(|gpu| is_gpu_idle(gpu, idle_memory_threshold_mb, idle_utilization_threshold))
        .map(|gpu| gpu.index)
        .collect();

    if devices.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "auto mode found no idle CUDA devices\n\n{}",
                idle_threshold_help(idle_memory_threshold_mb, idle_utilization_threshold)
            ),
        ))
    } else {
        Ok(devices)
    }
}

fn validate_explicit_cuda_devices(
    devices: &[usize],
    idle_memory_threshold_mb: usize,
    idle_utilization_threshold: usize,
) -> io::Result<Vec<usize>> {
    let snapshots = query_gpu_snapshots()?;
    let mut checked = Vec::new();

    for device in devices {
        let snapshot = snapshots
            .iter()
            .find(|gpu| gpu.index == *device)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("CUDA device {device} does not exist"),
                )
            })?;

        if !is_gpu_idle(
            snapshot,
            idle_memory_threshold_mb,
            idle_utilization_threshold,
        ) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                busy_cuda_device_message(
                    *device,
                    snapshot,
                    idle_memory_threshold_mb,
                    idle_utilization_threshold,
                ),
            ));
        }

        checked.push(*device);
    }

    Ok(checked)
}

fn query_gpu_snapshots() -> io::Result<Vec<GpuSnapshot>> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,memory.used,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(result) if result.status.success() => parse_gpu_snapshots(&result.stdout),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::Other,
            "failed to query GPUs with nvidia-smi",
        )),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "nvidia-smi is required for --cuda-devices auto or explicit validation",
        )),
        Err(err) => Err(err),
    }
}

fn parse_gpu_snapshots(stdout: &[u8]) -> io::Result<Vec<GpuSnapshot>> {
    let mut snapshots = Vec::new();
    for line in String::from_utf8_lossy(stdout).lines() {
        let parts: Vec<&str> = line.split(',').map(|part| part.trim()).collect();
        if parts.len() != 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected nvidia-smi output line: {line}"),
            ));
        }
        snapshots.push(GpuSnapshot {
            index: parts[0].parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid GPU index from nvidia-smi",
                )
            })?,
            memory_used_mb: parts[1].parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid memory.used from nvidia-smi",
                )
            })?,
            utilization_gpu: parts[2].parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid utilization.gpu from nvidia-smi",
                )
            })?,
        });
    }
    Ok(snapshots)
}

fn is_gpu_idle(
    snapshot: &GpuSnapshot,
    idle_memory_threshold_mb: usize,
    idle_utilization_threshold: usize,
) -> bool {
    snapshot.memory_used_mb <= idle_memory_threshold_mb
        && snapshot.utilization_gpu <= idle_utilization_threshold
}

fn busy_cuda_device_message(
    device: usize,
    snapshot: &GpuSnapshot,
    idle_memory_threshold_mb: usize,
    idle_utilization_threshold: usize,
) -> String {
    format!(
        "CUDA device {device} is busy (memory.used={} MiB, utilization.gpu={}%; thresholds: memory.used <= {} MiB, utilization.gpu <= {}%)\n\n{}\nThis GPU would pass the current check with --idle-memory-threshold-mb {} --idle-utilization-threshold {}.",
        snapshot.memory_used_mb,
        snapshot.utilization_gpu,
        idle_memory_threshold_mb,
        idle_utilization_threshold,
        idle_threshold_help(idle_memory_threshold_mb, idle_utilization_threshold),
        snapshot.memory_used_mb,
        snapshot.utilization_gpu
    )
}

fn idle_threshold_help(
    idle_memory_threshold_mb: usize,
    idle_utilization_threshold: usize,
) -> String {
    format!(
        "GPU idleness is checked before jobs start:\n  --idle-memory-threshold-mb N       allow a GPU only when memory.used <= N MiB (current: {idle_memory_threshold_mb})\n  --idle-utilization-threshold N     allow a GPU only when utilization.gpu <= N% (current: {idle_utilization_threshold})\n\nIf the memory is expected, for example from a harmless display process or cached context, you can relax the check:\n  --idle-memory-threshold-mb 5000 \\\n  --idle-utilization-threshold 40\n\nOnly relax these thresholds when you are sure the existing GPU activity will not conflict with your jobs."
    )
}

fn resolve_run_namespace(
    tmux: &TmuxClient,
    session: &str,
    current_window: &str,
    requested_name: Option<&str>,
    job_count: usize,
) -> io::Result<RunNamespace> {
    let windows = tmux.window_names(session)?;
    if let Some(name) = requested_name {
        let namespace = named_run_namespace(name);
        let conflicts = find_window_conflicts(
            &windows,
            current_window,
            &namespace.scheduler_window_name,
            &planned_window_names(&namespace, job_count),
        );
        return if conflicts.is_empty() {
            Ok(namespace)
        } else {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "window name conflict in session {session}: {}",
                    conflicts.join(", ")
                ),
            ))
        };
    }

    for idx in 1..=10_000 {
        let namespace = auto_run_namespace(idx);
        let conflicts = find_window_conflicts(
            &windows,
            current_window,
            &namespace.scheduler_window_name,
            &planned_window_names(&namespace, job_count),
        );
        if conflicts.is_empty() {
            return Ok(namespace);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("no available scheduler namespace in session {session}"),
    ))
}

fn auto_run_namespace(idx: usize) -> RunNamespace {
    if idx == 1 {
        RunNamespace {
            scheduler_window_name: DEFAULT_SCHEDULER_WINDOW_NAME.to_string(),
            job_window_prefix: DEFAULT_JOB_WINDOW_PREFIX.to_string(),
        }
    } else {
        RunNamespace {
            scheduler_window_name: format!(
                "{AUTO_SCHEDULER_WINDOW_PREFIX}{idx}{AUTO_SCHEDULER_WINDOW_SUFFIX}"
            ),
            job_window_prefix: format!("sched_{idx}_job"),
        }
    }
}

fn named_run_namespace(name: &str) -> RunNamespace {
    RunNamespace {
        scheduler_window_name: format!("__sched_{name}__"),
        job_window_prefix: format!("{name}_job"),
    }
}

fn window_name_for_job(job_window_prefix: &str, job_id: usize) -> String {
    format!("{job_window_prefix}_{job_id}")
}

pub struct Scheduler {
    session: String,
    jobs: Vec<Job>,
    logs_dir: PathBuf,
    job_window_prefix: String,
    cpu_threads: Option<usize>,
    device_ids: Vec<usize>,
    gpu_in_use: Vec<bool>,
    cpu_slots: Vec<Vec<usize>>,
    cpu_in_use: Vec<bool>,
    keep_job_tabs: bool,
    verbose: bool,
}

impl Scheduler {
    pub fn new(
        session: String,
        logs_dir: PathBuf,
        device_ids: Vec<usize>,
        cpu_slots: Vec<Vec<usize>>,
        commands: Vec<String>,
        job_window_prefix: String,
        cpu_threads: Option<usize>,
        keep_job_tabs: bool,
        verbose: bool,
    ) -> Self {
        let jobs = commands
            .into_iter()
            .enumerate()
            .map(|(idx, cmd)| Job::new(idx + 1, cmd, &logs_dir))
            .collect();
        let gpu_slots = device_ids.len();
        let cpu_slot_count = cpu_slots.len();
        Self {
            session,
            jobs,
            logs_dir,
            job_window_prefix,
            cpu_threads,
            device_ids,
            gpu_in_use: vec![false; gpu_slots],
            cpu_slots,
            cpu_in_use: vec![false; cpu_slot_count],
            keep_job_tabs,
            verbose,
        }
    }

    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    pub fn run_loop(&mut self, tick_interval: Duration) {
        loop {
            if let Err(err) = self.tick() {
                eprintln!("scheduler error: {err}");
            }
            if self.jobs.iter().all(|job| job.status.is_finished()) {
                break;
            }
            thread::sleep(tick_interval);
        }
    }

    pub fn summary(&self) -> String {
        let total = self.jobs.len();
        let done_ids = collect_job_ids(&self.jobs, JobStatus::Done);
        let failed_ids = collect_job_ids(&self.jobs, JobStatus::Failed);
        let cancelled_ids = collect_job_ids(&self.jobs, JobStatus::Cancelled);

        let mut lines = Vec::new();
        lines.push("===== Scheduler Summary =====".to_string());
        lines.push(format!("Session: {}", self.session));
        lines.push(format!(
            "CUDA devices: {}",
            format_cuda_devices(&self.device_ids)
        ));
        lines.push(format!("CPU slots: {}", format_cpu_slots(&self.cpu_slots)));
        lines.push(format!("Logs dir: {}", self.logs_dir.display()));
        lines.push(format!("Total jobs: {total}"));
        lines.push(format!("Done: {}", done_ids.len()));
        lines.push(format!("Failed: {}", failed_ids.len()));
        lines.push(format!("Cancelled: {}", cancelled_ids.len()));

        if !failed_ids.is_empty() {
            lines.push(format!("Failed job IDs: {}", join_ids(&failed_ids)));
        }
        if !cancelled_ids.is_empty() {
            lines.push(format!("Cancelled job IDs: {}", join_ids(&cancelled_ids)));
        }

        lines.join("\n")
    }

    pub fn tick(&mut self) -> io::Result<()> {
        self.schedule_pending_jobs()?;
        self.start_scheduled_jobs()?;
        self.finalize_running_jobs()?;
        Ok(())
    }

    fn schedule_pending_jobs(&mut self) -> io::Result<()> {
        for idx in 0..self.jobs.len() {
            if self.jobs[idx].status != JobStatus::Pending {
                continue;
            }
            let window_name = self.job_window_name(self.jobs[idx].id);
            ensure_job_window_absent(&TmuxClient, &self.session, &window_name)?;
            if let Some((gpu_id, cpu_cores)) = self.acquire_resources() {
                self.jobs[idx].gpu_id = gpu_id;
                self.jobs[idx].cpu_cores = cpu_cores;
                self.jobs[idx].status = JobStatus::Scheduled;
            }
        }
        Ok(())
    }

    fn start_scheduled_jobs(&mut self) -> io::Result<()> {
        for job in &mut self.jobs {
            if job.status != JobStatus::Scheduled {
                continue;
            }
            let window_name = window_name_for_job(&self.job_window_prefix, job.id);
            let script = build_script(
                job,
                job.gpu_id,
                job.cpu_cores.as_deref(),
                effective_cpu_threads(self.cpu_threads, job.cpu_cores.as_ref().map(Vec::len)),
            );
            TmuxClient.start_job_window(
                &self.session,
                &window_name,
                &script,
                self.keep_job_tabs,
            )?;
            job.window_name = Some(window_name);
            job.status = JobStatus::Running;
        }
        Ok(())
    }

    fn finalize_running_jobs(&mut self) -> io::Result<()> {
        for idx in 0..self.jobs.len() {
            if self.jobs[idx].status != JobStatus::Running {
                continue;
            }
            let window_name = self.jobs[idx]
                .window_name
                .as_deref()
                .expect("running jobs must have a window");
            let window_missing = !TmuxClient.has_window(&self.session, window_name)?;
            let pane_dead = if window_missing {
                false
            } else {
                TmuxClient.pane_dead(&self.session, window_name)?
            };
            if window_missing || pane_dead {
                self.finish_running_job(idx)?;
            }
        }
        Ok(())
    }

    fn finish_running_job(&mut self, idx: usize) -> io::Result<()> {
        let exit_code = read_exit_code(&self.jobs[idx].exit_path)?;
        self.jobs[idx].status = map_exit_code(exit_code);
        let status = self.jobs[idx].status.clone();
        let window_name = self.jobs[idx]
            .window_name
            .clone()
            .unwrap_or_else(|| self.job_window_name(self.jobs[idx].id));
        if let Some(gpu_id) = self.jobs[idx].gpu_id.take() {
            self.release_gpu(gpu_id);
        }
        if let Some(cpu_cores) = self.jobs[idx].cpu_cores.take() {
            self.release_cpu(&cpu_cores);
        }
        if self.verbose {
            println!(
                "[FINISHED] {} -> {}{}",
                window_name,
                format_job_status(&status),
                format_exit_code_suffix(exit_code)
            );
        }
        Ok(())
    }

    fn acquire_resources(&mut self) -> Option<(Option<usize>, Option<Vec<usize>>)> {
        let gpu_id = if self.device_ids.is_empty() {
            None
        } else {
            match self.acquire_gpu() {
                Some(gpu_id) => Some(gpu_id),
                None => return None,
            }
        };

        let cpu_cores = if self.cpu_slots.is_empty() {
            None
        } else {
            match self.acquire_cpu() {
                Some(cpu_cores) => Some(cpu_cores),
                None => {
                    if let Some(gpu_id) = gpu_id {
                        self.release_gpu(gpu_id);
                    }
                    return None;
                }
            }
        };

        Some((gpu_id, cpu_cores))
    }

    fn acquire_gpu(&mut self) -> Option<usize> {
        for (idx, in_use) in self.gpu_in_use.iter_mut().enumerate() {
            if !*in_use {
                *in_use = true;
                return self.device_ids.get(idx).copied();
            }
        }
        None
    }

    fn acquire_cpu(&mut self) -> Option<Vec<usize>> {
        for (idx, in_use) in self.cpu_in_use.iter_mut().enumerate() {
            if !*in_use {
                *in_use = true;
                return self.cpu_slots.get(idx).cloned();
            }
        }
        None
    }

    fn release_gpu(&mut self, gpu_id: usize) {
        if let Some(slot_idx) = self.device_ids.iter().position(|id| *id == gpu_id) {
            if let Some(slot) = self.gpu_in_use.get_mut(slot_idx) {
                *slot = false;
            }
        }
    }

    fn release_cpu(&mut self, cpu_cores: &[usize]) {
        if let Some(slot_idx) = self.cpu_slots.iter().position(|slot| slot == cpu_cores) {
            if let Some(slot) = self.cpu_in_use.get_mut(slot_idx) {
                *slot = false;
            }
        }
    }

    fn job_window_name(&self, job_id: usize) -> String {
        window_name_for_job(&self.job_window_prefix, job_id)
    }
}

fn format_cuda_devices(devices: &[usize]) -> String {
    if devices.is_empty() {
        return "none".to_string();
    }
    devices
        .iter()
        .map(|id| format!("cuda:{id}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn format_cpu_threads(cpu_threads: Option<usize>) -> String {
    cpu_threads
        .map(|threads| threads.to_string())
        .unwrap_or_else(|| "unlimited".to_string())
}

fn effective_cpu_threads(cpu_threads: Option<usize>, cpus_per_job: Option<usize>) -> Option<usize> {
    cpu_threads.or(cpus_per_job)
}

fn format_cpu_cores(cores: Option<&[usize]>) -> String {
    cores
        .map(format_cpu_core_list)
        .unwrap_or_else(|| "none".to_string())
}

fn format_cpu_core_list(cores: &[usize]) -> String {
    cores
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn format_cpu_slots(cpu_slots: &[Vec<usize>]) -> String {
    if cpu_slots.is_empty() {
        return "none".to_string();
    }
    cpu_slots
        .iter()
        .map(|slot| format!("[{}]", format_cpu_core_list(slot)))
        .collect::<Vec<_>>()
        .join(",")
}

fn build_dry_run_summary(
    options: &RunOptions,
    devices: &[usize],
    cpu_slots: &[Vec<usize>],
    job_count: usize,
) -> String {
    let input = options
        .input_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<stdin>".to_string());
    let mut lines = Vec::new();
    lines.push("===== Dry Run =====".to_string());
    lines.push(format!("Input: {input}"));
    lines.push(format!("Jobs: {job_count}"));
    lines.push(format!("CUDA devices: {}", format_cuda_devices(devices)));
    lines.push(format!(
        "CPU threads per job: {}",
        format_cpu_threads(effective_cpu_threads(
            options.cpu_threads,
            options.cpus_per_job
        ))
    ));
    lines.push(format!("CPU slots: {}", format_cpu_slots(cpu_slots)));
    lines.push(format!(
        "Idle memory threshold: {} MiB",
        options.idle_memory_threshold_mb
    ));
    lines.push(format!(
        "Idle utilization threshold: {}%",
        options.idle_utilization_threshold
    ));
    lines.push(format!("Logs dir: {}", options.logs_dir.display()));
    lines.push("tmux was not touched".to_string());
    lines.join("\n")
}

fn planned_window_names(namespace: &RunNamespace, job_count: usize) -> Vec<String> {
    std::iter::once(namespace.scheduler_window_name.clone())
        .chain((1..=job_count).map(|id| window_name_for_job(&namespace.job_window_prefix, id)))
        .collect()
}

fn find_window_conflicts(
    existing_windows: &[String],
    current_window: &str,
    scheduler_window_name: &str,
    planned_windows: &[String],
) -> Vec<String> {
    let scheduler_window_count = existing_windows
        .iter()
        .filter(|existing| existing.as_str() == scheduler_window_name)
        .count();
    let mut conflicts = Vec::new();

    for window in existing_windows {
        let is_conflict = if window.as_str() == scheduler_window_name {
            current_window != scheduler_window_name || scheduler_window_count > 1
        } else {
            planned_windows.iter().any(|planned| planned == window)
        };

        if is_conflict && !conflicts.contains(window) {
            conflicts.push(window.clone());
        }
    }

    conflicts
}

fn ensure_job_window_absent(tmux: &TmuxClient, session: &str, window_name: &str) -> io::Result<()> {
    if tmux.has_window(session, window_name)? {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("job window already exists in session {session}: {window_name}"),
        ))
    } else {
        Ok(())
    }
}

fn build_tmux_shell_command(script: &str) -> String {
    format!("bash -lc {}", shell_quote(script))
}

pub fn build_script(
    job: &Job,
    gpu_id: Option<usize>,
    cpu_cores: Option<&[usize]>,
    cpu_threads: Option<usize>,
) -> String {
    let gpu_display = gpu_id
        .map(|gpu_id| gpu_id.to_string())
        .unwrap_or_else(|| "none".to_string());
    let env_pairs = build_runtime_env_pairs(gpu_id, cpu_threads);
    let job_invocation = build_job_invocation(&job.cmd, cpu_cores);
    let cmd_display = format!("{} {}", format_env_pairs_inline(&env_pairs), job_invocation);
    let runtime_env = format_env_exports(&env_pairs);
    format!(
        r#"echo "================================"
echo "JOB ID: {job_id}"
echo "GPU: {gpu_display}"
echo "CPU CORES: {cpu_cores_display}"
echo "CPU THREADS: {cpu_threads_display}"
echo "LOG: {log_path}"
echo "================================"
echo "[CMD]"
echo {cmd_display_quoted}
echo "--------------------------------"

set -o pipefail

{runtime_env}
{job_invocation} \
2>&1 | tee {log_path_quoted}

EXIT_CODE=$?

echo "EXIT CODE: $EXIT_CODE"
echo $EXIT_CODE > {exit_path_quoted}

exit $EXIT_CODE"#,
        job_id = job.id,
        gpu_display = gpu_display,
        cpu_cores_display = format_cpu_cores(cpu_cores),
        cpu_threads_display = format_cpu_threads(cpu_threads),
        log_path = job.log_path.display(),
        cmd_display_quoted = shell_quote(&cmd_display),
        runtime_env = runtime_env,
        job_invocation = job_invocation,
        log_path_quoted = shell_quote_os(job.log_path.as_os_str()),
        exit_path_quoted = shell_quote_os(job.exit_path.as_os_str()),
    )
}

fn build_runtime_env_pairs(
    gpu_id: Option<usize>,
    cpu_threads: Option<usize>,
) -> Vec<(&'static str, String)> {
    let mut pairs = Vec::new();
    if let Some(gpu_id) = gpu_id {
        pairs.push(("CUDA_VISIBLE_DEVICES", gpu_id.to_string()));
    }
    pairs.push(("PYTHONUNBUFFERED", "1".to_string()));
    if let Some(cpu_threads) = cpu_threads {
        let value = cpu_threads.to_string();
        pairs.extend([
            ("OMP_NUM_THREADS", value.clone()),
            ("MKL_NUM_THREADS", value.clone()),
            ("OPENBLAS_NUM_THREADS", value.clone()),
            ("NUMEXPR_NUM_THREADS", value.clone()),
            ("VECLIB_MAXIMUM_THREADS", value.clone()),
            ("BLIS_NUM_THREADS", value.clone()),
            ("DIAMOND_TORCH_NUM_THREADS", value.clone()),
            ("DIAMOND_TORCH_INTEROP_THREADS", "1".to_string()),
        ]);
    }
    pairs
}

fn format_env_pairs_inline(pairs: &[(&'static str, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={}", shell_quote(value)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_env_exports(pairs: &[(&'static str, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("export {key}={}", shell_quote(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_job_invocation(cmd: &str, cpu_cores: Option<&[usize]>) -> String {
    let shell_cmd = format!("bash -lc {}", shell_quote(cmd));
    if let Some(cpu_cores) = cpu_cores {
        format!(
            "taskset -c {} {shell_cmd}",
            shell_quote(&format_cpu_core_list(cpu_cores))
        )
    } else {
        shell_cmd
    }
}

fn read_exit_code(path: &Path) -> io::Result<Option<i32>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let code = trimmed.parse::<i32>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid exit code in {}", path.display()),
        )
    })?;
    Ok(Some(code))
}

fn map_exit_code(exit_code: Option<i32>) -> JobStatus {
    match exit_code {
        Some(0) => JobStatus::Done,
        Some(130) | None => JobStatus::Cancelled,
        Some(_) => JobStatus::Failed,
    }
}

fn collect_job_ids(jobs: &[Job], target: JobStatus) -> Vec<usize> {
    jobs.iter()
        .filter(|job| job.status == target)
        .map(|job| job.id)
        .collect()
}

fn join_ids(ids: &[usize]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_job_status(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Pending => "Pending",
        JobStatus::Scheduled => "Scheduled",
        JobStatus::Running => "Running",
        JobStatus::Done => "Done",
        JobStatus::Failed => "Failed",
        JobStatus::Cancelled => "Cancelled",
    }
}

fn format_exit_code_suffix(exit_code: Option<i32>) -> String {
    match exit_code {
        Some(code) => format!(" (exit={code})"),
        None => String::new(),
    }
}

pub fn shell_quote(input: &str) -> String {
    if input.is_empty() {
        return "''".to_string();
    }
    let escaped = input.replace('\'', r#"'"'"'"#);
    format!("'{escaped}'")
}

fn shell_quote_os(value: &OsStr) -> String {
    shell_quote(&value.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_commands_skips_comments_and_empty_lines() {
        let input = "\n# comment\npython a.py\n  \npython b.py --x 1\n";
        let commands = parse_commands(input);
        assert_eq!(commands, vec!["python a.py", "python b.py --x 1"]);
    }

    #[test]
    fn parse_run_args_handles_options() {
        let args = vec![
            "tiny-exp-scheduler".to_string(),
            "run".to_string(),
            "commands.txt".to_string(),
            "--logs-dir".to_string(),
            "artifacts".to_string(),
            "--scheduler-name".to_string(),
            "train-a".to_string(),
            "--cpu-threads".to_string(),
            "2".to_string(),
            "--cpu-cores".to_string(),
            "0-3,8,10-11".to_string(),
            "--cpus-per-job".to_string(),
            "2".to_string(),
            "--cuda-devices".to_string(),
            "0,2".to_string(),
            "--idle-memory-threshold-mb".to_string(),
            "96".to_string(),
            "--idle-utilization-threshold".to_string(),
            "25".to_string(),
            "--dry-run".to_string(),
            "--keep-job-tabs".to_string(),
            "--verbose".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        match parsed {
            CliAction::Run(options) => {
                assert_eq!(options.input_path, Some(PathBuf::from("commands.txt")));
                assert_eq!(options.logs_dir, PathBuf::from("artifacts"));
                assert_eq!(options.scheduler_name, Some("train-a".to_string()));
                assert_eq!(options.cpu_threads, Some(2));
                assert_eq!(
                    options.cpu_cores,
                    CpuCoresArg::Explicit(vec![0, 1, 2, 3, 8, 10, 11])
                );
                assert_eq!(options.cpus_per_job, Some(2));
                assert_eq!(options.cuda_devices, CudaDevicesArg::Explicit(vec![0, 2]));
                assert_eq!(options.idle_memory_threshold_mb, 96);
                assert_eq!(options.idle_utilization_threshold, 25);
                assert!(options.dry_run);
                assert!(options.keep_job_tabs);
                assert!(options.verbose);
            }
            _ => panic!("expected run action"),
        }
    }

    #[test]
    fn help_text_explains_tmux_runtime_model() {
        let help = help_text();
        assert!(help.contains("Run inside an existing tmux session."));
        assert!(help.contains("The current tab becomes an available scheduler tab"));
        assert!(help.contains("By default, finished job tabs exit and disappear."));
        assert!(help.contains("Detect idle GPUs once at startup"));
        assert!(help.contains("--dry-run"));
        assert!(help.contains("--idle-memory-threshold-mb"));
        assert!(help.contains("--idle-utilization-threshold"));
        assert!(help.contains("--keep-job-tabs"));
        assert!(help.contains("--scheduler-name"));
        assert!(help.contains("--cpu-threads"));
        assert!(help.contains("--cpu-cores"));
        assert!(help.contains("--cpus-per-job"));
        assert!(help.contains("--verbose"));
        assert!(help.contains("tiny-exp-scheduler run [COMMANDS_FILE] [OPTIONS]"));
    }

    #[test]
    fn build_script_contains_required_pieces() {
        let job = Job::new(
            1,
            "python eval.py --ckpt 1000".to_string(),
            Path::new("logs"),
        );
        let script = build_script(&job, Some(1), None, None);
        assert!(script.contains("JOB ID: 1"));
        assert!(script.contains("GPU: 1"));
        assert!(script.contains("CPU CORES: none"));
        assert!(script.contains("CPU THREADS: unlimited"));
        assert!(script.contains("set -o pipefail"));
        assert!(script.contains("PYTHONUNBUFFERED='1'"));
        assert!(script.contains("tee 'logs/job_1.log'"));
        assert!(script.contains("echo $EXIT_CODE > 'logs/job_1.exit'"));
    }

    #[test]
    fn build_script_without_gpu_skips_cuda_visible_devices() {
        let job = Job::new(1, "python train.py".to_string(), Path::new("logs"));
        let script = build_script(&job, None, None, None);
        assert!(script.contains("GPU: none"));
        assert!(script.contains("PYTHONUNBUFFERED='1'"));
        assert!(!script.contains("CUDA_VISIBLE_DEVICES="));
    }

    #[test]
    fn build_script_with_cpu_threads_sets_thread_env() {
        let job = Job::new(1, "python train.py".to_string(), Path::new("logs"));
        let script = build_script(&job, Some(0), None, Some(2));
        assert!(script.contains("CPU THREADS: 2"));
        assert!(script.contains("CUDA_VISIBLE_DEVICES='0'"));
        assert!(script.contains("OMP_NUM_THREADS='2'"));
        assert!(script.contains("MKL_NUM_THREADS='2'"));
        assert!(script.contains("OPENBLAS_NUM_THREADS='2'"));
        assert!(script.contains("NUMEXPR_NUM_THREADS='2'"));
        assert!(script.contains("DIAMOND_TORCH_NUM_THREADS='2'"));
        assert!(script.contains("DIAMOND_TORCH_INTEROP_THREADS='1'"));
    }

    #[test]
    fn build_script_with_cpu_cores_wraps_command_in_taskset() {
        let job = Job::new(1, "python train.py --x 1".to_string(), Path::new("logs"));
        let cores = vec![0, 1, 2, 3];
        let script = build_script(&job, Some(0), Some(&cores), Some(4));
        assert!(script.contains("CPU CORES: 0,1,2,3"));
        assert!(script.contains("CPU THREADS: 4"));
        assert!(script.contains("taskset -c '0,1,2,3' bash -lc 'python train.py --x 1'"));
    }

    #[test]
    fn build_tmux_shell_command_wraps_script_as_one_argument() {
        let command = build_tmux_shell_command("echo 'hello'");
        assert_eq!(command, r#"bash -lc 'echo '"'"'hello'"'"''"#);
    }

    #[test]
    fn parse_args_rejects_internal_run_command() {
        let args = vec!["tiny-exp-scheduler".to_string(), "internal-run".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert!(err.contains("unknown command: internal-run"));
    }

    #[test]
    fn parse_cuda_devices_accepts_auto() {
        assert_eq!(
            parse_cuda_devices_arg("auto").unwrap(),
            CudaDevicesArg::Auto
        );
    }

    #[test]
    fn parse_cuda_devices_accepts_none() {
        assert_eq!(
            parse_cuda_devices_arg("none").unwrap(),
            CudaDevicesArg::None
        );
    }

    #[test]
    fn parse_cuda_devices_accepts_explicit_list() {
        assert_eq!(
            parse_cuda_devices_arg("2,0,2,1").unwrap(),
            CudaDevicesArg::Explicit(vec![0, 1, 2])
        );
    }

    #[test]
    fn parse_cpu_cores_accepts_ranges_and_lists() {
        assert_eq!(
            parse_cpu_cores_arg("4-6,1,6").unwrap(),
            CpuCoresArg::Explicit(vec![1, 4, 5, 6])
        );
    }

    #[test]
    fn split_cpu_pool_into_fixed_size_slots() {
        let slots = split_cpu_pool_into_slots(vec![0, 1, 2, 3, 4], 2).unwrap();
        assert_eq!(slots, vec![vec![0, 1], vec![2, 3]]);
    }

    #[test]
    fn resolve_cpu_slots_requires_cpus_per_job_for_explicit_pool() {
        let err = resolve_cpu_slots(&CpuCoresArg::Explicit(vec![0, 1]), None).unwrap_err();
        assert!(err.to_string().contains("--cpus-per-job"));
    }

    #[test]
    fn parse_gpu_snapshots_parses_nvidia_smi_output() {
        let snapshots = parse_gpu_snapshots(b"0, 12, 0\n1, 2048, 37\n").unwrap();
        assert_eq!(
            snapshots,
            vec![
                GpuSnapshot {
                    index: 0,
                    memory_used_mb: 12,
                    utilization_gpu: 0,
                },
                GpuSnapshot {
                    index: 1,
                    memory_used_mb: 2048,
                    utilization_gpu: 37,
                },
            ]
        );
    }

    #[test]
    fn idle_gpu_rule_matches_current_threshold() {
        assert!(is_gpu_idle(
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 64,
                utilization_gpu: 0,
            },
            64,
            0
        ));
        assert!(!is_gpu_idle(
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 65,
                utilization_gpu: 0,
            },
            64,
            0
        ));
        assert!(!is_gpu_idle(
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 32,
                utilization_gpu: 1,
            },
            64,
            0
        ));
        assert!(is_gpu_idle(
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 96,
                utilization_gpu: 0,
            },
            96,
            0
        ));
        assert!(is_gpu_idle(
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 96,
                utilization_gpu: 10,
            },
            96,
            10
        ));
    }

    #[test]
    fn busy_cuda_device_message_explains_idle_thresholds() {
        let message = busy_cuda_device_message(
            0,
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 4484,
                utilization_gpu: 0,
            },
            64,
            0,
        );

        assert!(message.contains("CUDA device 0 is busy"));
        assert!(message.contains("--idle-memory-threshold-mb 5000"));
        assert!(message.contains("--idle-utilization-threshold 40"));
        assert!(message.contains("allow a GPU only when memory.used <= N MiB"));
        assert!(message.contains("allow a GPU only when utilization.gpu <= N%"));
        assert!(message.contains("--idle-memory-threshold-mb 4484 --idle-utilization-threshold 0"));
    }

    #[test]
    fn format_cuda_devices_is_explicit() {
        assert_eq!(format_cuda_devices(&[0, 2, 5]), "cuda:0,cuda:2,cuda:5");
    }

    #[test]
    fn format_cuda_devices_none_is_stable() {
        assert_eq!(format_cuda_devices(&[]), "none");
    }

    #[test]
    fn shell_quote_handles_single_quotes() {
        let quoted = shell_quote("python -c 'print(1)'");
        assert_eq!(quoted, r#"'python -c '"'"'print(1)'"'"''"#);
    }

    #[test]
    fn scheduler_summary_lists_counts_and_problem_ids() {
        let scheduler = Scheduler {
            session: "exp".to_string(),
            jobs: vec![
                Job {
                    id: 1,
                    cmd: "a".to_string(),
                    status: JobStatus::Done,
                    gpu_id: None,
                    cpu_cores: None,
                    window_name: Some("job_1".to_string()),
                    log_path: PathBuf::from("logs/job_1.log"),
                    exit_path: PathBuf::from("logs/job_1.exit"),
                },
                Job {
                    id: 2,
                    cmd: "b".to_string(),
                    status: JobStatus::Failed,
                    gpu_id: None,
                    cpu_cores: None,
                    window_name: Some("job_2".to_string()),
                    log_path: PathBuf::from("logs/job_2.log"),
                    exit_path: PathBuf::from("logs/job_2.exit"),
                },
                Job {
                    id: 3,
                    cmd: "c".to_string(),
                    status: JobStatus::Cancelled,
                    gpu_id: None,
                    cpu_cores: None,
                    window_name: Some("job_3".to_string()),
                    log_path: PathBuf::from("logs/job_3.log"),
                    exit_path: PathBuf::from("logs/job_3.exit"),
                },
            ],
            logs_dir: PathBuf::from("logs"),
            job_window_prefix: DEFAULT_JOB_WINDOW_PREFIX.to_string(),
            cpu_threads: None,
            device_ids: vec![0, 1, 2],
            gpu_in_use: vec![false, false, false],
            cpu_slots: vec![vec![0, 1], vec![2, 3]],
            cpu_in_use: vec![false, false],
            keep_job_tabs: false,
            verbose: false,
        };

        let summary = scheduler.summary();
        assert!(summary.contains("Session: exp"));
        assert!(summary.contains("CUDA devices: cuda:0,cuda:1,cuda:2"));
        assert!(summary.contains("CPU slots: [0,1],[2,3]"));
        assert!(summary.contains("Logs dir: logs"));
        assert!(summary.contains("Total jobs: 3"));
        assert!(summary.contains("Done: 1"));
        assert!(summary.contains("Failed: 1"));
        assert!(summary.contains("Cancelled: 1"));
        assert!(summary.contains("Failed job IDs: 2"));
        assert!(summary.contains("Cancelled job IDs: 3"));
    }

    #[test]
    fn scheduler_releases_gpu_after_finish() {
        let mut scheduler = Scheduler::new(
            "session".to_string(),
            PathBuf::from("logs"),
            vec![3],
            Vec::new(),
            vec!["python a.py".to_string(), "python b.py".to_string()],
            DEFAULT_JOB_WINDOW_PREFIX.to_string(),
            None,
            false,
            false,
        );
        scheduler.jobs[0].status = JobStatus::Running;
        scheduler.jobs[0].gpu_id = Some(3);
        scheduler.gpu_in_use[0] = true;
        scheduler.release_gpu(3);
        scheduler.jobs[0].status = JobStatus::Done;
        assert!(!scheduler.gpu_in_use[0]);
    }

    #[test]
    fn map_exit_code_matches_spec() {
        assert_eq!(map_exit_code(Some(0)), JobStatus::Done);
        assert_eq!(map_exit_code(Some(130)), JobStatus::Cancelled);
        assert_eq!(map_exit_code(Some(1)), JobStatus::Failed);
        assert_eq!(map_exit_code(None), JobStatus::Cancelled);
    }

    #[test]
    fn ctrl_c_exit_code_marks_job_cancelled_and_releases_gpu() {
        let logs_dir = unique_test_dir("ctrl-c");
        fs::create_dir_all(&logs_dir).unwrap();
        let mut scheduler = Scheduler::new(
            "session".to_string(),
            logs_dir.clone(),
            vec![0],
            Vec::new(),
            vec!["python train.py".to_string()],
            DEFAULT_JOB_WINDOW_PREFIX.to_string(),
            None,
            false,
            false,
        );
        scheduler.jobs[0].status = JobStatus::Running;
        scheduler.jobs[0].gpu_id = Some(0);
        scheduler.gpu_in_use[0] = true;
        fs::write(scheduler.jobs[0].exit_path.clone(), "130\n").unwrap();
        scheduler.finish_running_job(0).unwrap();
        assert_eq!(scheduler.jobs[0].status, JobStatus::Cancelled);
        assert_eq!(scheduler.jobs[0].gpu_id, None);
        assert!(!scheduler.gpu_in_use[0]);
        fs::remove_dir_all(logs_dir).unwrap();
    }

    #[test]
    fn missing_exit_file_marks_closed_window_as_cancelled_and_releases_gpu() {
        let logs_dir = unique_test_dir("window-closed");
        fs::create_dir_all(&logs_dir).unwrap();
        let mut scheduler = Scheduler::new(
            "session".to_string(),
            logs_dir.clone(),
            vec![0],
            Vec::new(),
            vec!["python train.py".to_string()],
            DEFAULT_JOB_WINDOW_PREFIX.to_string(),
            None,
            false,
            false,
        );
        scheduler.jobs[0].status = JobStatus::Running;
        scheduler.jobs[0].gpu_id = Some(0);
        scheduler.gpu_in_use[0] = true;
        scheduler.finish_running_job(0).unwrap();
        assert_eq!(scheduler.jobs[0].status, JobStatus::Cancelled);
        assert_eq!(scheduler.jobs[0].gpu_id, None);
        assert!(!scheduler.gpu_in_use[0]);
        fs::remove_dir_all(logs_dir).unwrap();
    }

    #[test]
    fn rename_current_window_command_is_stable() {
        // This only verifies the public name contract used by docs and runtime.
        assert_eq!(DEFAULT_SCHEDULER_WINDOW_NAME, "__sched__");
    }

    #[test]
    fn dry_run_summary_includes_gpu_range_and_logs_dir() {
        let options = RunOptions {
            input_path: Some(PathBuf::from("commands.txt")),
            logs_dir: PathBuf::from("artifacts"),
            scheduler_name: None,
            cpu_threads: Some(2),
            cpu_cores: CpuCoresArg::Explicit(vec![0, 1, 2, 3]),
            cpus_per_job: Some(2),
            cuda_devices: CudaDevicesArg::Explicit(vec![0, 2]),
            idle_memory_threshold_mb: 96,
            idle_utilization_threshold: 25,
            tick_seconds: 1,
            dry_run: true,
            keep_job_tabs: false,
            verbose: false,
        };
        let summary = build_dry_run_summary(&options, &[0, 2], &[vec![0, 1], vec![2, 3]], 4);
        assert!(summary.contains("===== Dry Run ====="));
        assert!(summary.contains("Jobs: 4"));
        assert!(summary.contains("CUDA devices: cuda:0,cuda:2"));
        assert!(summary.contains("CPU threads per job: 2"));
        assert!(summary.contains("CPU slots: [0,1],[2,3]"));
        assert!(summary.contains("Idle memory threshold: 96 MiB"));
        assert!(summary.contains("Idle utilization threshold: 25%"));
        assert!(summary.contains("Logs dir: artifacts"));
    }

    #[test]
    fn find_window_conflicts_detects_scheduler_and_job_tabs() {
        let namespace = auto_run_namespace(1);
        let planned = planned_window_names(&namespace, 3);
        let existing = vec![
            "shell".to_string(),
            "__sched__".to_string(),
            "job_2".to_string(),
        ];
        let conflicts = find_window_conflicts(
            &existing,
            "shell",
            &namespace.scheduler_window_name,
            &planned,
        );
        assert_eq!(
            conflicts,
            vec!["__sched__".to_string(), "job_2".to_string()]
        );
    }

    #[test]
    fn find_window_conflicts_allows_current_scheduler_window() {
        let namespace = auto_run_namespace(1);
        let planned = planned_window_names(&namespace, 1);
        let existing = vec!["__sched__".to_string(), "shell".to_string()];
        let conflicts = find_window_conflicts(
            &existing,
            "__sched__",
            &namespace.scheduler_window_name,
            &planned,
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn find_window_conflicts_rejects_duplicate_scheduler_window() {
        let namespace = auto_run_namespace(1);
        let planned = planned_window_names(&namespace, 1);
        let existing = vec![
            "__sched__".to_string(),
            "shell".to_string(),
            "__sched__".to_string(),
        ];
        let conflicts = find_window_conflicts(
            &existing,
            "__sched__",
            &namespace.scheduler_window_name,
            &planned,
        );
        assert_eq!(conflicts, vec!["__sched__".to_string()]);
    }

    #[test]
    fn find_window_conflicts_still_rejects_job_tabs_from_current_scheduler_window() {
        let namespace = auto_run_namespace(1);
        let planned = planned_window_names(&namespace, 1);
        let existing = vec!["__sched__".to_string(), "job_1".to_string()];
        let conflicts = find_window_conflicts(
            &existing,
            "__sched__",
            &namespace.scheduler_window_name,
            &planned,
        );
        assert_eq!(conflicts, vec!["job_1".to_string()]);
    }

    #[test]
    fn find_window_conflicts_allows_other_scheduler_job_tabs() {
        let namespace = auto_run_namespace(2);
        let planned = planned_window_names(&namespace, 2);
        let existing = vec!["shell".to_string(), "job_99".to_string()];
        let conflicts = find_window_conflicts(
            &existing,
            "shell",
            &namespace.scheduler_window_name,
            &planned,
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn find_window_conflicts_rejects_current_scheduler_namespaced_job_tabs() {
        let namespace = auto_run_namespace(2);
        let planned = planned_window_names(&namespace, 2);
        let existing = vec![
            "job_1".to_string(),
            "sched_2_job_1".to_string(),
            "sched_3_job_1".to_string(),
        ];
        let conflicts = find_window_conflicts(
            &existing,
            "shell",
            &namespace.scheduler_window_name,
            &planned,
        );
        assert_eq!(conflicts, vec!["sched_2_job_1".to_string()]);
    }

    #[test]
    fn planned_window_names_match_runtime_contract() {
        let namespace = auto_run_namespace(1);
        assert_eq!(
            planned_window_names(&namespace, 3),
            vec![
                "__sched__".to_string(),
                "job_1".to_string(),
                "job_2".to_string(),
                "job_3".to_string(),
            ]
        );
    }

    #[test]
    fn planned_window_names_for_auto_secondary_scheduler_are_namespaced() {
        let namespace = auto_run_namespace(2);
        assert_eq!(
            planned_window_names(&namespace, 2),
            vec![
                "__sched_2__".to_string(),
                "sched_2_job_1".to_string(),
                "sched_2_job_2".to_string(),
            ]
        );
    }

    #[test]
    fn planned_window_names_for_named_scheduler_are_namespaced() {
        let namespace = named_run_namespace("train-a");
        assert_eq!(
            planned_window_names(&namespace, 2),
            vec![
                "__sched_train-a__".to_string(),
                "train-a_job_1".to_string(),
                "train-a_job_2".to_string(),
            ]
        );
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("tiny-exp-scheduler-{label}-{nanos}"))
    }
}
