# Design Document

Start with [README.md](../README.md) for installation and basic usage. See
[cli.md](cli.md) for option definitions and [workflows.md](workflows.md) for representative command patterns.

## 1. System Overview

`tiny-exp-scheduler` is a single-machine scheduler for running shell commands in parallel on multiple GPUs.

Its default resource model is one running job per GPU. That makes it primarily a scheduler for
single-GPU jobs, with an explicit `none` mode for jobs that should not receive any CUDA allocation.
Optionally, it can also allocate fixed CPU core slots and launch jobs under `taskset`.

At a high level, the tool does four things:

1. read a list of *job*s
2. resolve the GPU set for this run
3. launch one running *job* per `tmux` tab
4. record logs and final status

A *job* is one non-empty, non-comment input line. Each *job* is one complete shell command.

## 2. Top-Down Abstractions

The system can be understood as five layers.

### Layer 1: Command Source

The scheduler reads input from either:

- a command file
- standard input

The input format is line-oriented:

- ignore empty lines
- ignore lines starting with `#`
- treat each remaining line as one *job*

The command file uses shell syntax. It is not a shell script to be executed directly.

### Layer 2: Scheduler Core

The scheduler core owns:

- the pending-job queue
- the running-job set
- the fixed GPU pool for this run
- the optional fixed CPU core slot pool for this run
- the main scheduling loop

Its job is to map pending *job*s onto available GPUs and to track state transitions until completion.

### Layer 3: tmux Runtime

The scheduler runs inside an existing `tmux` session.

Within that session:

- the current tab is renamed to an available scheduler tab, such as `s`
- each running *job* gets one new tab in that scheduler's namespace
- each job tab contains one pane
- that pane runs one shell command

So the runtime shape is:

```text
command source
    |
    v
s
    |
    +--> j1
    +--> j2
    +--> j3
    +--> ...

s2
    |
    +--> s2j1
    +--> s2j2
```

### Layer 4: Job Execution

A running *job* is a shell command materialized with scheduler-controlled environment and logging.

Conceptually, each job is launched as:

```text
CUDA_VISIBLE_DEVICES=<gpu_id> + raw shell command + log capture + exit capture
```

The scheduler controls GPU visibility from the outside. The job command itself remains user-defined.

### Layer 5: Persistent Output

For each *job*, the scheduler writes:

- one log file
- one exit-status file

These files are the persistent record of execution, independent of whether the corresponding `tmux` tab remains visible.

## 3. Architecture

The architecture has five runtime components.

```text
command file / stdin
        |
        v
  input parser
        |
        v
 scheduler core
    |       |
    |       +--> GPU pool manager
    |
    +--> tmux tab launcher
    |
    +--> status collector
    |
    +--> logs / exit files
```

Their responsibilities are:

- *input parser*: normalize lines into jobs
- *GPU pool manager*: determine which GPUs this run may use
- *scheduler core*: assign jobs to free GPUs
- *tmux tab launcher*: materialize running jobs in tabs
- *status collector*: detect completion and derive final state

## 4. Workflow

The runtime workflow has three phases.

### Phase 1: Startup

At startup, the scheduler:

1. checks that it is running inside `tmux`
2. reads jobs from file or stdin
3. resolves the GPU set for this run
4. validates startup conditions
5. resolves a tmux namespace for this run
6. renames the current tab to that scheduler tab

Startup rejects invalid states such as:

- not running inside `tmux`
- requested scheduler or job tab names already existing in the current session
- no usable GPU under the requested mode
- an explicitly requested GPU already being busy

### Phase 2: Scheduling Loop

After startup, the scheduler enters a polling loop.

Each iteration does the following:

```text
1. find free GPUs inside the fixed GPU pool
2. find free CPU slots inside the fixed CPU pool, if enabled
3. assign pending jobs only when all requested resources are available
4. launch newly assigned jobs in tmux tabs
5. check running jobs for completion
6. reclaim GPUs and CPU slots from finished jobs
7. sleep until the next tick
```

This loop continues until no pending or running jobs remain.

### Phase 3: Shutdown

When all jobs are done, the scheduler:

1. computes final job states
2. prints the summary in the scheduler tab
3. keeps the scheduler tab open

Finished job tabs either disappear immediately or remain visible, depending on `--keep-job-tabs`.

## 5. GPU Workflow

GPU handling is based on a fixed pool model.

### Step 1: Resolve the GPU pool

The scheduler supports three modes:

```text
--cuda-devices auto
--cuda-devices none
--cuda-devices 0,2,5
```

If `auto` is used:

- inspect GPUs through `nvidia-smi` at startup
- collect the currently idle GPUs
- freeze that set as the GPU pool for this run

If an explicit list is used:

- use exactly the listed GPUs
- fail if any listed GPU is busy at startup

If `none` is used:

- the scheduler does not allocate GPUs
- the scheduler does not set `CUDA_VISIBLE_DEVICES`
- jobs are not limited by GPU-slot count

### Step 2: Use only that pool during scheduling

The scheduler never expands the pool later. GPUs that become idle after startup are ignored unless they were already part of the resolved pool.

### Step 3: Reclaim GPUs from finished jobs

When a running job finishes, its GPU returns to the free subset of the same pool and may be assigned to another pending job.

The idle rule used by `auto` is:

```text
memory.used <= threshold
and
utilization.gpu <= utilization_threshold
```

The memory threshold is controlled by `--idle-memory-threshold-mb`.
The utilization threshold is controlled by `--idle-utilization-threshold`.
Both conditions must be satisfied for a GPU to be treated as idle.

## 6. CPU Workflow

CPU allocation is disabled by default. When enabled, the scheduler treats CPU cores as fixed-size slots.

```text
--cpu-cores 0-31
--cpus-per-job 8
```

This creates slots `0-7`, `8-15`, `16-23`, and `24-31`. A pending job starts only when a CPU slot is free. The launched command is wrapped as:

```text
taskset -c <slot> bash -lc '<raw command>'
```

The affinity applies to the command's subprocesses, including PyTorch DataLoader workers. The scheduler also sets common OpenMP/BLAS/PyTorch thread environment variables. If `--cpu-threads` is omitted, the thread count defaults to `--cpus-per-job`; otherwise `--cpu-threads` overrides it.

## 7. Job Workflow

Each job has two kinds of states:

- execution states, used by the scheduler loop
- final result states, shown to the user after execution ends

The execution-state machine is:

```text
Pending -> Scheduled -> Running -> Finished
```

The final result states are:

- `Done`
- `Failed`
- `Cancelled`

So the full picture is:

```text
Pending -> Scheduled -> Running -> Finished -> {Done | Failed | Cancelled}
```

Here, `Finished` only means that the command is no longer running. After that, the scheduler derives the final result from `tmux` runtime state and the exit file.

### Pending

The job has been parsed but has not yet been assigned a GPU.

### Scheduled

The scheduler has assigned a GPU to the job, but the job has not yet been launched in `tmux`.

### Running

The job has been launched in its own `tmux` tab and is currently occupying one GPU.

Operationally, a job is considered `Running` after its tab and pane have been created and the command has been started there.

### Finished

A job reaches `Finished` when the scheduler determines that the command is no longer alive.

This is detected from `tmux` state in either of these cases:

- the job tab no longer exists
- the job pane is dead, even if the tab still exists

Once a job reaches `Finished`, the scheduler derives its final result as follows:

- if the exit file exists and the exit code is `0`, the job becomes `Done`
- if the exit file exists and the exit code is `130`, the job becomes `Cancelled`
- if the exit file exists and the exit code is any other non-zero value, the job becomes `Failed`
- if the job tab is gone and no exit file exists, the job becomes `Cancelled`

So `Done`, `Failed`, and `Cancelled` are not parallel to `Running`; they are final classifications assigned after `Finished`.

## 8. tmux Workflow

The scheduler uses `tmux` as the runtime container and as part of job-state detection.

### Scheduler tab

The current tab becomes a scheduler tab. The first default scheduler uses `s`; additional default schedulers in the same tmux session use names such as `s2`. With `--scheduler-name NAME`, the scheduler tab is `s.NAME`.

The scheduler tab hosts the control loop and the final summary.

### Job tabs

Each launched job gets one tab in the scheduler namespace. The first default scheduler uses `jX`; additional default schedulers use names such as `s2jX`. With `--scheduler-name NAME`, jobs use `NAME.jX`.

Each job tab contains one pane, and that pane runs one command.

### State detection through tmux

For each running job, the scheduler tracks the corresponding `tmux` tab and pane.

At each scheduler tick, it checks whether:

- the tab still exists
- the pane is still alive

These checks are used to determine whether the job is still running or has reached `Finished`.

The interpretation is:

- tab exists and pane is alive -> the job is still `Running`
- tab exists but pane is dead -> the job has reached `Finished`
- tab is missing -> the job has reached `Finished`

After that, the scheduler consults the exit file to derive `Done`, `Failed`, or `Cancelled`.

### Completion behavior

By default:

- finished job tabs exit and disappear

With `--keep-job-tabs`:

- finished job tabs remain visible for inspection
- the pane process has still exited
- the job has still reached `Finished`

This means tab visibility and job liveness are not the same thing: with `--keep-job-tabs`, a tab may remain visible even though the job is no longer running.

## 9. Command Materialization

The scheduler does not interpret job semantics, but it does materialize each raw command into a concrete runtime form.

Conceptually, each job launch adds three things around the raw command:

1. GPU visibility control through `CUDA_VISIBLE_DEVICES`
2. log capture
3. exit-status capture

So the runtime form is roughly:

```text
scheduler env + raw shell command + stdout/stderr capture + exit capture
```

This is the only place where the scheduler wraps the user command.

## 10. Status and Output Model

The default output shape is:

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

Final state is derived as follows:

- exit code `0` -> `Done`
- exit code `130` -> `Cancelled`
- other non-zero exit code -> `Failed`
- missing exit file with a missing tab -> `Cancelled`

So the scheduler uses both process-exit information and `tmux` runtime state.

## 11. Interrupt Workflow

Interrupts enter the system through `tmux` or through process exit.

### Ctrl+C inside a job tab

- the command usually exits with `130`
- the job becomes `Cancelled`
- the GPU is reclaimed

### Job tab killed manually

- the tab disappears
- if no exit file exists, the job becomes `Cancelled`
- the GPU is reclaimed

### Whole tmux session killed

- all running jobs are treated as ended
- jobs without exit files become `Cancelled`
- all GPUs are reclaimed
