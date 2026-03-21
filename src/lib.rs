use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const SCHEDULER_WINDOW_NAME: &str = "__scheduler__";

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

    fn start_job_window(&self, session: &str, window_name: &str, script: &str) -> io::Result<()> {
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
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("failed to start tmux window: {window_name}"),
            ))
        }
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
    pub cuda_devices: CudaDevicesArg,
    pub idle_memory_threshold_mb: usize,
    pub tick_seconds: u64,
    pub dry_run: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            input_path: None,
            logs_dir: PathBuf::from("logs"),
            cuda_devices: CudaDevicesArg::Auto,
            idle_memory_threshold_mb: 64,
            tick_seconds: 1,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CudaDevicesArg {
    Auto,
    Explicit(Vec<usize>),
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
  tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices auto]
  tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices 0,2,5]
  tiny-exp-scheduler run < commands.txt
  tiny-exp-scheduler -h | --help

MODEL:
  Run inside an existing tmux session.
  The current tab becomes __scheduler__.
  Job tabs are appended after it.

  current tab
      |
      +--> __scheduler__
      +--> job_1
      +--> job_2
      +--> ...

OPTIONS:
  --logs-dir DIR      Directory for log and exit-code files. Default: logs
  --cuda-devices ARG  'auto' or a comma-separated list like 0,2,5.
                      Default: auto
  --idle-memory-threshold-mb N
                      Max memory.used for an idle GPU. Default: 64
  --tick-seconds N    Scheduler polling interval in seconds. Default: 1
  --dry-run           Resolve GPUs and print the plan without touching tmux.
  -h, --help          Show this help message.

CUDA DEVICES:
  auto                Detect idle GPUs once at startup and freeze that set.
  0,2,5               Use exactly these GPUs; all must already be idle.
  idle rule           memory.used <= threshold and utilization.gpu == 0
  startup output      Prints the final adopted CUDA device range.

INPUT RULES:
  - ignore empty lines
  - ignore lines starting with '#'
  - each remaining line is one job

SCHEDULER LOOP:
  loop:
    schedule pending jobs onto the frozen CUDA pool
    start scheduled jobs in tmux tabs
    finalize jobs whose tabs disappeared
    sleep(tick_seconds)
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

    let gpu_devices =
        resolve_cuda_devices(&options.cuda_devices, options.idle_memory_threshold_mb)?;
    if gpu_devices.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no usable CUDA devices resolved",
        ));
    }

    if options.dry_run {
        println!(
            "{}",
            build_dry_run_summary(&options, &gpu_devices, commands.len())
        );
        return Ok(());
    }

    tmux.ensure_available()?;
    ensure_running_inside_tmux()?;
    fs::create_dir_all(&options.logs_dir)?;

    let session = tmux.current_session()?;
    ensure_window_plan_absent(&tmux, &session, commands.len())?;
    tmux.rename_current_window(SCHEDULER_WINDOW_NAME)?;

    println!("Session: {session}");
    println!("Scheduler tab: {SCHEDULER_WINDOW_NAME}");
    println!("Current tab renamed to {SCHEDULER_WINDOW_NAME}.");
    println!("Job tabs will be appended after it.");
    println!(
        "Final CUDA device range: {}",
        format_cuda_devices(&gpu_devices)
    );
    println!("Logs dir: {}", options.logs_dir.display());
    let mut scheduler = Scheduler::new(session, options.logs_dir, gpu_devices, commands);
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

fn parse_cuda_devices_arg(value: &str) -> Result<CudaDevicesArg, String> {
    if value == "auto" {
        return Ok(CudaDevicesArg::Auto);
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

pub fn resolve_cuda_devices(
    selection: &CudaDevicesArg,
    idle_memory_threshold_mb: usize,
) -> io::Result<Vec<usize>> {
    match selection {
        CudaDevicesArg::Auto => resolve_auto_cuda_devices(idle_memory_threshold_mb),
        CudaDevicesArg::Explicit(devices) => {
            validate_explicit_cuda_devices(devices, idle_memory_threshold_mb)
        }
    }
}

fn resolve_auto_cuda_devices(idle_memory_threshold_mb: usize) -> io::Result<Vec<usize>> {
    let snapshots = query_gpu_snapshots()?;
    let devices: Vec<usize> = snapshots
        .into_iter()
        .filter(|gpu| is_gpu_idle(gpu, idle_memory_threshold_mb))
        .map(|gpu| gpu.index)
        .collect();

    if devices.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "auto mode found no idle CUDA devices",
        ))
    } else {
        Ok(devices)
    }
}

fn validate_explicit_cuda_devices(
    devices: &[usize],
    idle_memory_threshold_mb: usize,
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

        if !is_gpu_idle(snapshot, idle_memory_threshold_mb) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "CUDA device {device} is busy (memory.used={} MiB, utilization.gpu={}%)",
                    snapshot.memory_used_mb, snapshot.utilization_gpu
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

fn is_gpu_idle(snapshot: &GpuSnapshot, idle_memory_threshold_mb: usize) -> bool {
    snapshot.memory_used_mb <= idle_memory_threshold_mb && snapshot.utilization_gpu == 0
}

fn ensure_window_plan_absent(tmux: &TmuxClient, session: &str, job_count: usize) -> io::Result<()> {
    let windows = tmux.window_names(session)?;
    let conflicts = find_window_conflicts(&windows, job_count);
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "window name conflict in session {session}: {}",
                conflicts.join(", ")
            ),
        ))
    }
}

pub struct Scheduler {
    session: String,
    jobs: Vec<Job>,
    logs_dir: PathBuf,
    device_ids: Vec<usize>,
    gpu_in_use: Vec<bool>,
}

impl Scheduler {
    pub fn new(
        session: String,
        logs_dir: PathBuf,
        device_ids: Vec<usize>,
        commands: Vec<String>,
    ) -> Self {
        let jobs = commands
            .into_iter()
            .enumerate()
            .map(|(idx, cmd)| Job::new(idx + 1, cmd, &logs_dir))
            .collect();
        let slots = device_ids.len();
        Self {
            session,
            jobs,
            logs_dir,
            device_ids,
            gpu_in_use: vec![false; slots],
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
            if let Some(gpu_id) = self.acquire_gpu() {
                let window_name = self.jobs[idx].name();
                ensure_job_window_absent(&TmuxClient, &self.session, &window_name)?;
                self.jobs[idx].gpu_id = Some(gpu_id);
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
            let gpu_id = job.gpu_id.expect("scheduled jobs must own a GPU");
            let window_name = job.name();
            let script = build_script(job, gpu_id);
            TmuxClient.start_job_window(&self.session, &window_name, &script)?;
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
            if !TmuxClient.has_window(&self.session, window_name)? {
                self.finish_running_job(idx)?;
            }
        }
        Ok(())
    }

    fn finish_running_job(&mut self, idx: usize) -> io::Result<()> {
        let exit_code = read_exit_code(&self.jobs[idx].exit_path)?;
        self.jobs[idx].status = map_exit_code(exit_code);
        if let Some(gpu_id) = self.jobs[idx].gpu_id.take() {
            self.release_gpu(gpu_id);
        }
        Ok(())
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

    fn release_gpu(&mut self, gpu_id: usize) {
        if let Some(slot_idx) = self.device_ids.iter().position(|id| *id == gpu_id) {
            if let Some(slot) = self.gpu_in_use.get_mut(slot_idx) {
                *slot = false;
            }
        }
    }
}

fn format_cuda_devices(devices: &[usize]) -> String {
    devices
        .iter()
        .map(|id| format!("cuda:{id}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn build_dry_run_summary(options: &RunOptions, devices: &[usize], job_count: usize) -> String {
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
        "Idle memory threshold: {} MiB",
        options.idle_memory_threshold_mb
    ));
    lines.push(format!("Logs dir: {}", options.logs_dir.display()));
    lines.push("tmux was not touched".to_string());
    lines.join("\n")
}

fn planned_window_names(job_count: usize) -> Vec<String> {
    std::iter::once(SCHEDULER_WINDOW_NAME.to_string())
        .chain((1..=job_count).map(|id| format!("job_{id}")))
        .collect()
}

fn find_window_conflicts(existing_windows: &[String], job_count: usize) -> Vec<String> {
    planned_window_names(job_count)
        .into_iter()
        .filter(|name| existing_windows.iter().any(|existing| existing == name))
        .collect()
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

pub fn build_script(job: &Job, gpu_id: usize) -> String {
    let cmd_display = format!("CUDA_VISIBLE_DEVICES={gpu_id} {}", job.cmd);
    format!(
        r#"echo "================================"
echo "JOB ID: {job_id}"
echo "GPU: {gpu_id}"
echo "LOG: {log_path}"
echo "================================"
echo "[CMD]"
echo {cmd_display_quoted}
echo "--------------------------------"

set -o pipefail

CUDA_VISIBLE_DEVICES={gpu_id} \
PYTHONUNBUFFERED=1 \
{cmd} \
2>&1 | tee {log_path_quoted}

EXIT_CODE=$?

echo "EXIT CODE: $EXIT_CODE"
echo $EXIT_CODE > {exit_path_quoted}

exit $EXIT_CODE"#,
        job_id = job.id,
        gpu_id = gpu_id,
        log_path = job.log_path.display(),
        cmd_display_quoted = shell_quote(&cmd_display),
        cmd = job.cmd,
        log_path_quoted = shell_quote_os(job.log_path.as_os_str()),
        exit_path_quoted = shell_quote_os(job.exit_path.as_os_str()),
    )
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
            "--cuda-devices".to_string(),
            "0,2".to_string(),
            "--idle-memory-threshold-mb".to_string(),
            "96".to_string(),
            "--dry-run".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        match parsed {
            CliAction::Run(options) => {
                assert_eq!(options.input_path, Some(PathBuf::from("commands.txt")));
                assert_eq!(options.logs_dir, PathBuf::from("artifacts"));
                assert_eq!(options.cuda_devices, CudaDevicesArg::Explicit(vec![0, 2]));
                assert_eq!(options.idle_memory_threshold_mb, 96);
                assert!(options.dry_run);
            }
            _ => panic!("expected run action"),
        }
    }

    #[test]
    fn help_text_explains_tmux_runtime_model() {
        let help = help_text();
        assert!(help.contains("Run inside an existing tmux session."));
        assert!(help.contains("The current tab becomes __scheduler__."));
        assert!(help.contains("Detect idle GPUs once at startup"));
        assert!(help.contains("--dry-run"));
        assert!(help.contains("--idle-memory-threshold-mb"));
    }

    #[test]
    fn build_script_contains_required_pieces() {
        let job = Job::new(
            1,
            "python eval.py --ckpt 1000".to_string(),
            Path::new("logs"),
        );
        let script = build_script(&job, 1);
        assert!(script.contains("JOB ID: 1"));
        assert!(script.contains("GPU: 1"));
        assert!(script.contains("set -o pipefail"));
        assert!(script.contains("PYTHONUNBUFFERED=1"));
        assert!(script.contains("tee 'logs/job_1.log'"));
        assert!(script.contains("echo $EXIT_CODE > 'logs/job_1.exit'"));
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
    fn parse_cuda_devices_accepts_explicit_list() {
        assert_eq!(
            parse_cuda_devices_arg("2,0,2,1").unwrap(),
            CudaDevicesArg::Explicit(vec![0, 1, 2])
        );
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
            64
        ));
        assert!(!is_gpu_idle(
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 65,
                utilization_gpu: 0,
            },
            64
        ));
        assert!(!is_gpu_idle(
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 32,
                utilization_gpu: 1,
            },
            64
        ));
        assert!(is_gpu_idle(
            &GpuSnapshot {
                index: 0,
                memory_used_mb: 96,
                utilization_gpu: 0,
            },
            96
        ));
    }

    #[test]
    fn format_cuda_devices_is_explicit() {
        assert_eq!(format_cuda_devices(&[0, 2, 5]), "cuda:0,cuda:2,cuda:5");
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
                    window_name: Some("job_1".to_string()),
                    log_path: PathBuf::from("logs/job_1.log"),
                    exit_path: PathBuf::from("logs/job_1.exit"),
                },
                Job {
                    id: 2,
                    cmd: "b".to_string(),
                    status: JobStatus::Failed,
                    gpu_id: None,
                    window_name: Some("job_2".to_string()),
                    log_path: PathBuf::from("logs/job_2.log"),
                    exit_path: PathBuf::from("logs/job_2.exit"),
                },
                Job {
                    id: 3,
                    cmd: "c".to_string(),
                    status: JobStatus::Cancelled,
                    gpu_id: None,
                    window_name: Some("job_3".to_string()),
                    log_path: PathBuf::from("logs/job_3.log"),
                    exit_path: PathBuf::from("logs/job_3.exit"),
                },
            ],
            logs_dir: PathBuf::from("logs"),
            device_ids: vec![0, 1, 2],
            gpu_in_use: vec![false, false, false],
        };

        let summary = scheduler.summary();
        assert!(summary.contains("Session: exp"));
        assert!(summary.contains("CUDA devices: cuda:0,cuda:1,cuda:2"));
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
            vec!["python a.py".to_string(), "python b.py".to_string()],
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
            vec!["python train.py".to_string()],
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
            vec!["python train.py".to_string()],
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
        assert_eq!(SCHEDULER_WINDOW_NAME, "__scheduler__");
    }

    #[test]
    fn dry_run_summary_includes_gpu_range_and_logs_dir() {
        let options = RunOptions {
            input_path: Some(PathBuf::from("commands.txt")),
            logs_dir: PathBuf::from("artifacts"),
            cuda_devices: CudaDevicesArg::Explicit(vec![0, 2]),
            idle_memory_threshold_mb: 96,
            tick_seconds: 1,
            dry_run: true,
        };
        let summary = build_dry_run_summary(&options, &[0, 2], 4);
        assert!(summary.contains("===== Dry Run ====="));
        assert!(summary.contains("Jobs: 4"));
        assert!(summary.contains("CUDA devices: cuda:0,cuda:2"));
        assert!(summary.contains("Idle memory threshold: 96 MiB"));
        assert!(summary.contains("Logs dir: artifacts"));
    }

    #[test]
    fn find_window_conflicts_detects_scheduler_and_job_tabs() {
        let existing = vec![
            "shell".to_string(),
            "__scheduler__".to_string(),
            "job_2".to_string(),
        ];
        let conflicts = find_window_conflicts(&existing, 3);
        assert_eq!(
            conflicts,
            vec!["__scheduler__".to_string(), "job_2".to_string()]
        );
    }

    #[test]
    fn planned_window_names_match_runtime_contract() {
        assert_eq!(
            planned_window_names(3),
            vec![
                "__scheduler__".to_string(),
                "job_1".to_string(),
                "job_2".to_string(),
                "job_3".to_string(),
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
