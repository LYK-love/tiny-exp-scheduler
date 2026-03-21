[中文](README.md) | [English](README.en.md)

# tiny-exp-scheduler

`tiny-exp-scheduler` is a lightweight scheduler for single-machine GPU experiments. You give it a list of shell commands, and it launches them in tmux, assigns available GPUs, and writes each command's output to log files.

It does not generate commands and it does not interpret command meaning. It handles a small, explicit set of responsibilities:

- treat each input line as one job
- occupy the current tmux tab as the scheduler window
- open one new tab per job in the same tmux session
- decide which GPUs are allowed for this run at startup, then schedule only within that set
- record logs and final status for each job

For deeper details, see the [design document](docs/design.en.md).  
For runnable examples, see [workflow demos](docs/workflows.en.md).

## Mental Model

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

The allowed GPU range is decided only once at startup:

```text
--cuda-devices auto
    |
    +--> check which GPUs are idle at startup
    +--> record them as the GPUs allowed for this run
    +--> use only that set for the whole run
```

## Installation

Requirements:

- Rust 1.74+
- `tmux`
- `nvidia-smi`

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

```bash
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices auto]
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices 0,2,5]
tiny-exp-scheduler run [commands.txt] [--dry-run]
cat commands.txt | tiny-exp-scheduler run --cuda-devices auto
```

Before running:

- you must already be inside a tmux session and sitting in one tab

After startup:

- the current tab is renamed to `__scheduler__`
- job tabs are appended after it
- the `__scheduler__` tab is kept after completion
- if `__scheduler__` already exists in the current session, the command fails

GPU selection:

- `--cuda-devices auto`
  inspect GPUs through `nvidia-smi` at startup, then use the currently idle GPUs as the allowed device list for this run
- `--cuda-devices 0,2,5`
  use exactly those GPUs; if any one of them is already busy, the command fails immediately
- `--idle-memory-threshold-mb N`
  adjust the memory threshold used when deciding whether a GPU counts as idle; default `64`
- the current idle rule is:
  `memory.used <= threshold` and `utilization.gpu == 0`
- startup prints the final adopted range, for example:
  `Final CUDA device range: cuda:0,cuda:2,cuda:5`
- `--dry-run`
  read input, inspect GPUs, and print the plan without touching tmux tabs or starting jobs

Input rules:

- ignore empty lines
- ignore lines starting with `#`
- each remaining line is one job

## Minimal Example

Enter tmux first:

```bash
tmux new-session -s exp
```

Then run:

```bash
tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices 0
```

More deep-learning-like examples:

- [examples/torch_hold_gpu.py](examples/torch_hold_gpu.py)
- [examples/torch-two-gpu-jobs.txt](examples/torch-two-gpu-jobs.txt)
- [examples/torch-four-gpu-jobs.txt](examples/torch-four-gpu-jobs.txt)

These use minimal `torch` code to allocate about 2000 MB on one GPU and hold it for tens of seconds.

## Logs and Status

Default output:

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

After a job ends, the program derives its state from the [exit status / exit code](https://en.wikipedia.org/wiki/Exit_status):

- `0` => `Done`
  the command finished normally
- `130` => `Cancelled`
  typically caused by `Ctrl+C` inside a job tab
- any other non-zero => `Failed`
  the command exited with an error
- missing window and missing `.exit` => `Cancelled`
  typically caused by killing the job tab directly or killing the whole session

At the end, the `__scheduler__` tab prints a summary with:

- final CUDA device range
- logs directory
- total job count
- Done / Failed / Cancelled counts
- Failed / Cancelled job ids

## Tests

```bash
cargo test
bash scripts/tmux-smoke.sh
```

## License

MIT

This project was written collaboratively by AI and humans.
