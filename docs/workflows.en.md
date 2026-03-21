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

## 3. Deep-learning-style GPU hold

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

## 4. Running outside tmux

Command:

```bash
./target/release/tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices auto
```

Expected:

- immediate error
- message says an existing tmux session is required

## 5. Dry Run

Command:

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices auto --dry-run
```

Expected:

- print the final CUDA range
- print the job count and logs directory
- do not rename the current tmux tab
- do not start any jobs

## 6. Ctrl+C in one job

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

## 7. Kill one job tab

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

## 8. Kill the whole session

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

## 9. Read tasks from stdin

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
