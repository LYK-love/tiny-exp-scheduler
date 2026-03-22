# Workflow Demos

Start with [README.en.md](../README.en.md) for installation and basic usage. See
[design.en.md](design.en.md) for the runtime model and state rules behind these examples.

## Prerequisites

```bash
cd tiny-exp-scheduler
tmux new-session -s exp
```

For the GPU demos you also need:

- `torch` installed
- `torch.cuda.is_available()` to be true
- usable GPUs on the machine

## 1. Minimal Queue

Command:

```bash
tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices 0
```

Meaning:

- one allowed GPU
- one job runs at a time
- `job_2` waits for `job_1`

Check:

```bash
cat logs/job_1.exit
cat logs/job_2.exit
```

Both should be `0`.

## 2. Multi-GPU Concurrency

Command:

```bash
tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

Meaning:

- four allowed GPUs
- four jobs can start in parallel
- each running job gets its own tmux tab

## 3. One Script Invocation Per Line

This is the common pattern for real experiments: shared setup stays in one script, while
`commands.txt` only carries run-specific arguments.

Example `commands.txt`:

```text
bash scripts/train_one.sh exp_a configs/pong_a.yaml
bash scripts/train_one.sh exp_b configs/pong_b.yaml
bash scripts/train_one.sh exp_c configs/pong_c.yaml
```

Example wrapper script:

```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

export PYTHONPATH="${PROJECT_ROOT}/src"
export DATASET_PATH="${PROJECT_ROOT}/dataset/pong/train"

RUN_NAME="$1"
CONFIG_PATH="$2"

python src/main.py \
  --config "${CONFIG_PATH}" \
  --dataset-path "${DATASET_PATH}" \
  --run-name "${RUN_NAME}"
```

Command:

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
```

Meaning:

- each input line is still one shell command
- shared environment stays in the wrapper script
- `CUDA_VISIBLE_DEVICES` comes from the scheduler, not from the script

## 4. GPU Occupancy Demo

Single-GPU queueing:

```bash
tiny-exp-scheduler run examples/torch-two-gpu-jobs.txt --cuda-devices 0
```

Expected:

- `job_1` holds the GPU first
- `job_2` waits
- `nvidia-smi` shows extra GPU memory usage

Four-GPU concurrency:

```bash
tiny-exp-scheduler run examples/torch-four-gpu-jobs.txt --cuda-devices 0,1,2,3
```

Expected:

- `job_1` through `job_4` start together
- each job binds one GPU

## 5. Dry Run

Command:

```bash
tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices auto --dry-run
```

Meaning:

- resolve the input
- resolve the CUDA device range
- print the plan
- do not rename the current tmux tab
- do not launch any jobs

## 6. Interrupt One Job

Command:

```bash
tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

Action:

- switch to a running job tab
- press `Ctrl+C`

Expected:

- that job becomes `Cancelled`
- its `.exit` file becomes `130`
- its GPU is released
- other jobs continue

Alternative action:

```bash
tmux kill-window -t exp:job_3
```

This also cancels that one job and releases its GPU.

This project was written collaboratively by AI and humans.
