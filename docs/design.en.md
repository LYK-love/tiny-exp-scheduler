# Design Document

## Project Role

`tiny-exp-scheduler` is a small scheduler for single-machine GPU experiments. It takes a list of ready-made shell commands, turns the current tmux tab into the scheduler window, creates one task tab per command in the same tmux session, and assigns those tasks over a preselected set of GPUs.

## System View

```text
commands.txt
    |
    v
tmux session
    |
    +--> current tab -> __scheduler__
    +--> appended tab -> job_1
    +--> appended tab -> job_2
    +--> ...
```

Once started, the program roughly follows this sequence:

```text
tiny-exp-scheduler run ...
    |
    +--> confirm that it is running inside tmux
    +--> read the command list
    +--> decide which GPUs are allowed for this run
    +--> rename the current tab to __scheduler__
    +--> enter the polling loop
```

## Design Principles

- execute the command the user wrote
- do not interpret training arguments, model names, or script semantics
- do not generate commands or expand parameters
- only solve queueing, GPU placement, and logging
- keep the implementation small and explicit

## Input

Input sources:

- file input: `tiny-exp-scheduler run commands.txt`
- standard input: `cat commands.txt | tiny-exp-scheduler run`

The input format is intentionally simple:

- ignore empty lines
- ignore lines starting with `#`
- each remaining line is one job

## tmux Runtime Model

Constraints:

- users must already be inside an existing tmux session and sitting in one tab
- the program does not create sessions
- the current tab is renamed to `__scheduler__`
- each job gets a new tab named `job_X`
- `__scheduler__` is kept by default
- only one `__scheduler__` may exist per session

## GPU Selection and Assignment

The CLI uses one option to control the GPU range:

```text
--cuda-devices auto
--cuda-devices 0,2,5
```

If `auto` is used:

- inspect GPUs through `nvidia-smi` at startup
- record the currently idle GPUs as the device list allowed for this run
- GPUs that become idle later are not added to that running scheduler

If an explicit list is used:

- use exactly the GPU ids named by the user
- the program checks that all of them are currently idle
- if any one is busy, the command fails
- the range is never silently reduced

The current idle check is:

```text
memory.used <= threshold
utilization.gpu == 0
```

Here `threshold` is controlled by `--idle-memory-threshold-mb`, with a default of `64`. In other words, a GPU is considered idle only when its memory usage is at most 64 MiB and its GPU utilization is zero.

Why the GPU list is fixed once at startup:

```text
startup:
  --cuda-devices auto
  idle GPUs found -> [0,2,5]

runtime:
  scheduler only allocates from [0,2,5]
  GPU 3 becoming idle later does not change the pool

This keeps the resource boundary stable for a single run instead of letting it drift as other workloads on the machine change over time.
```

## Job State Machine

```text
Pending
  -> Scheduled
  -> Running
  -> Done / Failed / Cancelled
```

The states mean:

- `Pending`: no GPU yet
- `Scheduled`: GPU assigned, waiting to start
- `Running`: tmux job tab created
- `Done`: exit status / exit code `0`
- `Failed`: non-zero other than `130`
- `Cancelled`: exit status `130`, or a missing window with no `.exit`

## Scheduler Loop

The scheduler wakes up periodically and does this:

```text
loop:
  assign available GPUs to jobs that have not started yet
  start jobs that already have a GPU assigned
  detect running jobs whose tabs have disappeared
  sleep(tick_seconds)
```

The order is fixed:

```text
1. Pending -> Scheduled
2. Scheduled -> Running
3. Running -> Finished
```

## Actual Command Execution

Each job is materialized as an explicit shell script like this:

```bash
CUDA_VISIBLE_DEVICES=<gpu_id> \
PYTHONUNBUFFERED=1 \
<raw command> \
2>&1 | tee logs/job_X.log
```

Besides running the raw command, the script also:

- prints job metadata
- enables `set -o pipefail`
- writes `logs/job_X.exit`

## Logs and Final State

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

After a job ends, the program derives its state from the [exit status / exit code](https://en.wikipedia.org/wiki/Exit_status):

- `0` -> `Done`
  the command finished normally
- `130` -> `Cancelled`
  usually caused by user `Ctrl+C`
- any other non-zero -> `Failed`
  the command exited with an error
- missing `.exit` with a missing window -> `Cancelled`
  usually caused by killing a job tab directly or killing the whole session

## Common Interrupt Scenarios

`Ctrl+C`:

- usually writes `130`
- job becomes `Cancelled`
- GPU is released

`kill-window`:

- window disappears
- if `.exit` is missing, treat as `Cancelled`
- GPU is released

`kill-session`:

- all running jobs are treated as ended
- jobs without `.exit` become `Cancelled`
- GPUs are released

## Current CLI

```bash
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices auto] [--tick-seconds N]
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices 0,2,5] [--tick-seconds N]
tiny-exp-scheduler run [commands.txt] [--dry-run]
```

Extra notes:

- `--dry-run` only parses input and resolves the final CUDA range; it does not touch tmux tabs
- the scheduler summary prints the final CUDA range and logs directory

This project was written collaboratively by AI and humans.
