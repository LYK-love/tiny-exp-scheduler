# Workflow Demos

## Prerequisites

```bash
cargo build --release
cd tiny-exp-scheduler
tmux new-session -s exp
```

For `torch` demos you also need:

- `torch` installed
- `torch.cuda.is_available()` to be true
- available GPUs on the machine

Quick flow:

```text
launch tab
  -> __scheduler__
  -> job_1
  -> job_2
  -> ...
```

## 1. Minimal run

Command:

```bash
./target/release/tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices 0
```

Expected:

- current tab becomes `__scheduler__`
- `job_1` runs first
- `job_2` starts after `job_1` finishes

Check:

```bash
cat logs/job_1.exit
cat logs/job_2.exit
```

Both should be:

```text
0
```

## 2. Four-way concurrency

Command:

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

Expected:

- current tab becomes `__scheduler__`
- `job_1` through `job_4` appear
- all four jobs start almost at once

## 3. One Script Invocation Per Line

This is the most common pattern when experiments share environment setup.

Wrapper script:

```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

export PYTHONPATH="${PROJECT_ROOT}/src"
export DATASET_PATH="${PROJECT_ROOT}/dataset/pong/test"
export BACKEND_ENDPOINT="http://209.137.198.192:7263"

python "${PROJECT_ROOT}/src/tools/run_experiment.py" \
  --dataset-path "${DATASET_PATH}" \
  --backend-endpoint "${BACKEND_ENDPOINT}" \
  --run-name "$1"
```

`commands.txt`:

```text
bash scripts/run_experiment.sh exp_a
bash scripts/run_experiment.sh exp_b
bash scripts/run_experiment.sh exp_c
bash scripts/run_experiment.sh exp_d
```

Command:

```bash
./target/release/tiny-exp-scheduler run commands.txt --cuda-devices 0,1,2,3
```

Expected:

- each input line is still one complete shell command
- shared setup stays in one script instead of being duplicated four times
- `CUDA_VISIBLE_DEVICES` is inherited inside `run_experiment.sh`
- the scheduler still controls which GPU each task gets

## 4. Deep-learning-style GPU hold

Single-GPU queueing:

```bash
./target/release/tiny-exp-scheduler run examples/torch-two-gpu-jobs.txt --cuda-devices 0
```

Expected:

- `job_1` occupies the GPU first
- `job_2` waits
- `nvidia-smi` shows about 2000 MB extra usage

Four-GPU concurrency:

```bash
./target/release/tiny-exp-scheduler run examples/torch-four-gpu-jobs.txt --cuda-devices 0,1,2,3
```

Expected:

- `job_1` through `job_4` start together
- each job binds one GPU

## 5. Running outside tmux

Command:

```bash
./target/release/tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices auto
```

Expected:

- immediate error
- message says an existing tmux session is required

## 6. Dry Run

Command:

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices auto --dry-run
```

Expected:

- print the final CUDA range
- print the job count and logs directory
- do not rename the current tmux tab
- do not start any jobs

## 7. Ctrl+C in one job

Command:

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

Action:

- switch to a running job tab
- press `Ctrl+C`

Expected:

- that job becomes `Cancelled`
- its `.exit` file becomes `130`
- its GPU is released
- other jobs continue

Check:

```bash
cat logs/job_2.exit
```

## 8. Kill one job tab

Command:

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

Action:

```bash
tmux kill-window -t exp:job_3
```

Expected:

- `job_3` becomes `Cancelled`
- its GPU is released
- other jobs continue

## 9. Kill the whole session

Command:

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

Action:

```bash
tmux kill-session -t exp
```

Expected:

- all running jobs are treated as ended
- jobs without `.exit` become `Cancelled`
- all GPUs are released

## 10. Read tasks from stdin

Command:

```bash
cat examples/basic-queue.txt | ./target/release/tiny-exp-scheduler run --cuda-devices auto
```

Meaning:

- equivalent to passing a file path
- only the input source changes

## Minimal Manual Regression Set

```text
1. basic-queue + explicit single GPU
2. four-jobs + explicit multi-GPU
3. torch-two-gpu-jobs
4. Ctrl+C on one job
5. tmux kill-window
6. tmux kill-session
```

This project was written collaboratively by AI and humans.
