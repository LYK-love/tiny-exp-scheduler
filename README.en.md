[中文](README.md) | [English](README.en.md)

# tiny-exp-scheduler

`tiny-exp-scheduler` is a minimal [*tmux*-based](#terminology) GPU *job* scheduler for a single machine with multiple graphics processing units (GPUs).

It does one thing: given a list of shell commands, it assigns available GPUs, launches the commands in parallel in separate [*tmux tabs*](#terminology), and records logs and exit status.

It does not generate commands, define a domain-specific language (DSL), or manage distributed systems. Each input line is just one shell command, and the scheduler is responsible only for GPU assignment, *tmux* launching, and [job](#terminology) tracking.

## Execution Model

```text
commands.txt
    |
    v
__scheduler__ tab
    |
    +--> job_1 tab  (GPU a)
    +--> job_2 tab  (GPU b)
    +--> job_3 tab  (GPU c)
    +--> ...
```

The *allowed GPU set* is decided once at startup and is used for the whole run.

## Scheduling Logic

```text
read jobs
select allowed GPUs
while unfinished jobs exist:
    wait for an allowed GPU to become free
    launch next job in a new tmux tab
    set CUDA_VISIBLE_DEVICES for that job
    record log and exit status
print final summary
```

## Installation

Requirements:

- Rust 1.74+
- `tmux`
- NVIDIA System Management Interface (`nvidia-smi`)

Build:

```bash
git clone git@github.com:LYK-love/tiny-exp-scheduler.git
cd tiny-exp-scheduler
cargo build --release
```

Binary:

```bash
target/release/tiny-exp-scheduler
```

## Usage

Before running, you must already be inside a *tmux session* and sitting in one *tmux tab*.

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
tiny-exp-scheduler run commands.txt --cuda-devices 0,2,5
tiny-exp-scheduler run commands.txt --logs-dir logs
tiny-exp-scheduler run commands.txt --dry-run
cat commands.txt | tiny-exp-scheduler run --cuda-devices auto
```

At startup:

- the current *tmux tab* is renamed to `__scheduler__`
- *job* tabs are appended after it
- if `__scheduler__` already exists in the current *tmux session*, the command fails

At completion:

- the `__scheduler__` tab remains
- the scheduler prints a final summary there

## GPU Selection

- `--cuda-devices auto`  
  At startup, inspect all GPUs and keep the idle ones as the *allowed GPU set* for this run.

- `--cuda-devices 0,2,5`  
  Use exactly the listed GPUs. If any listed GPU is busy at startup, the command fails.

Idle rule:

```text
memory.used <= threshold
and
utilization.gpu == 0
```

Default threshold: `64` megabytes (MB).

Adjust it with:

```bash
--idle-memory-threshold-mb N
```

At startup, the scheduler prints the final adopted GPU set, for example:

```text
Final CUDA device range: cuda:0,cuda:2,cuda:5
```

- `--dry-run`  
  Read input, inspect GPUs, and print the plan without touching *tmux* tabs or starting *job*s.

## Input Rules

- follow the syntax of shell
- ignore empty lines and lines starting with `#`, i.e., comments
- treat each remaining line as one *job*

Example:

```text
# commands.txt
python train.py --exp exp_a
python train.py --exp exp_b
python train.py --exp exp_c
```

## Script Pattern (Wrapper Script)

The recommended pattern is: keep each input line as one shell command, and let that command invoke one script.

Put shared environment setup in a *wrapper script*. Do not set `CUDA_VISIBLE_DEVICES` in that script. Let the scheduler set it from the outside.

Pass run-specific arguments from `commands.txt` into the script, for example through `$1`, `$2`, and so on.

Example `commands.txt`:

```text
bash scripts/run_experiment.sh exp_a
bash scripts/run_experiment.sh exp_b
bash scripts/run_experiment.sh exp_c
bash scripts/run_experiment.sh exp_d
```



Example *wrapper script*:

```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

export PYTHONPATH="${PROJECT_ROOT}/src"
export DATASET_PATH="${PROJECT_ROOT}/dataset/pong/test"
export BACKEND_ENDPOINT="http://localhost:8080"

python "${PROJECT_ROOT}/src/tools/run_experiment.py" \
  --dataset-path "${DATASET_PATH}" \
  --backend-endpoint "${BACKEND_ENDPOINT}" \
  --run-name "$1"
```

`CUDA_VISIBLE_DEVICES` is set by the scheduler and inherited by the *job* process, so it shouldn't be specified in the wrapper script.

## Minimal Example

Start *tmux*:

```bash
tmux new-session -s exp
```

Run:

```bash
tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices 0
```

## Logs and Status

Default output:

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

*Job* state is derived from exit status:

- `0` -> `Done`
- `130` -> `Cancelled`
- other non-zero -> `Failed`

If a *job* tab disappears and no `.exit` file is found, the scheduler marks it as `Cancelled`.

At the end, the `__scheduler__` tab prints:

- final CUDA device range
- logs directory
- total *job* count
- counts of `Done` / `Failed` / `Cancelled`
- ids of `Failed` / `Cancelled` *job*s

## More

- [design document](docs/design.en.md)
- [workflow demos](docs/workflows.en.md)
- [examples/torch_hold_gpu.py](examples/torch_hold_gpu.py)
- [examples/torch-two-gpu-jobs.txt](examples/torch-two-gpu-jobs.txt)
- [examples/torch-four-gpu-jobs.txt](examples/torch-four-gpu-jobs.txt)

## Tests

```bash
cargo test
bash scripts/tmux-smoke.sh
```

## Terminology

- [*tmux*](https://github.com/tmux/tmux/wiki): a terminal multiplexer.
- *job*: one non-empty, non-comment input line; each *job* is one complete shell command.
- *tmux session*: the current *tmux* session in which the scheduler runs.
- *tmux tab*: the term used in this README for a *tmux* window.
- *allowed GPU set*: the set of GPUs that this run may use. It is decided once at startup.

## License

MIT
