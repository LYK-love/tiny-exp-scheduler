# Design Document

## One Sentence

`tiny-exp-scheduler` maps line-based shell commands into independent job tabs inside the current tmux session and schedules them over a frozen CUDA device range with a simple polling loop.

## System View

```text
commands.txt
    |
    v
current tmux tab
    |
    +--> __scheduler__
    +--> job_1
    +--> job_2
    +--> ...
```

Startup path:

```text
tiny-exp-scheduler run ...
    |
    +--> check tmux context
    +--> resolve CUDA device range
    +--> rename current tab -> __scheduler__
    +--> enter polling scheduler loop
```

## Core Principles

- input is command
- do not parse command meaning
- do not generate commands
- only schedule commands
- keep the scheduler simple

## Input Model

Input sources:

- file input: `tiny-exp-scheduler run commands.txt`
- standard input: `cat commands.txt | tiny-exp-scheduler run`

Rules:

- ignore empty lines
- ignore lines starting with `#`
- each remaining line is one job

## tmux Model

Constraints:

- users must already be inside an existing tmux session
- the program does not create sessions
- the current tab is renamed to `__scheduler__`
- each job gets a new tab named `job_X`
- `__scheduler__` is kept by default
- only one `__scheduler__` may exist per session

## GPU Model

There is only one option:

```text
--cuda-devices auto
--cuda-devices 0,2,5
```

`auto`:

- find currently idle GPUs through `nvidia-smi` at startup
- freeze that set as the device pool for this run
- GPUs that become idle later are not added

Explicit list:

- use exactly the GPU ids named by the user
- the program checks that all of them are currently idle
- if any one is busy, the command fails
- the range is never silently reduced

Current idle rule:

```text
memory.used <= threshold
utilization.gpu == 0
```

Here `threshold` is controlled by `--idle-memory-threshold-mb`, default `64`.

Frozen-pool semantics:

```text
startup:
  --cuda-devices auto
  idle GPUs found -> [0,2,5]

runtime:
  scheduler only allocates from [0,2,5]
  GPU 3 becoming idle later does not change the pool
```

## Job State Machine

```text
Pending
  -> Scheduled
  -> Running
  -> Done / Failed / Cancelled
```

Meaning:

- `Pending`: no GPU yet
- `Scheduled`: GPU assigned, waiting to start
- `Running`: tmux job tab created
- `Done`: exit code `0`
- `Failed`: non-zero other than `130`
- `Cancelled`: exit code `130`, or missing window with no `.exit`

## Scheduler Loop

Pseudocode:

```text
loop:
  schedule pending jobs onto frozen CUDA pool
  start scheduled jobs in tmux tabs
  finalize running jobs whose tabs disappeared
  sleep(tick_seconds)
```

Fixed order:

```text
1. Pending -> Scheduled
2. Scheduled -> Running
3. Running -> Finished
```

## Execution Model

Each job is materialized as an explicit shell script:

```bash
CUDA_VISIBLE_DEVICES=<gpu_id> \
PYTHONUNBUFFERED=1 \
<raw command> \
2>&1 | tee logs/job_X.log
```

The script also:

- prints job metadata
- enables `set -o pipefail`
- writes `logs/job_X.exit`

## Logs and Exit Codes

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

Mapping:

- `0` -> `Done`
- `130` -> `Cancelled`
- any other non-zero -> `Failed`
- missing `.exit` with a missing window -> `Cancelled`

## Edge Cases

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
