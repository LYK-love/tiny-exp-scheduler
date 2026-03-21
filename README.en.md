[中文](README.md) | [English](README.en.md)

# tiny-exp-scheduler

`tiny-exp-scheduler` is a minimal, explicit tmux scheduler for single-machine GPU / deep learning workloads.

It does not generate commands and it does not interpret command meaning. It only does three things:

- treat each input line as one shell job
- open one tmux tab per job in the current session
- schedule those jobs over a frozen CUDA device range and write logs

For deeper details, see the [design document](docs/design.en.md).  
For runnable examples, see [workflow demos](docs/workflows.en.md).

## Mental Model

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

GPU selection happens once at startup:

```text
--cuda-devices auto
    |
    +--> detect idle GPUs once
    +--> freeze that set
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

Runtime requirements:

- you must already be inside a tmux session
- the current tab is renamed to `__scheduler__`
- job tabs are appended after it
- the `__scheduler__` tab is kept after completion
- if `__scheduler__` already exists in the current session, the command fails

CUDA selection:

- `--cuda-devices auto`
  detect idle GPUs via `nvidia-smi` at startup and freeze that set
- `--cuda-devices 0,2,5`
  use exactly those GPUs; if any one is not idle, the command fails
- `--idle-memory-threshold-mb N`
  adjust the `memory.used` cap in the idle rule; default `64`
- current idle rule:
  `memory.used <= threshold` and `utilization.gpu == 0`
- startup prints the final adopted range, for example:
  `Final CUDA device range: cuda:0,cuda:2,cuda:5`
- `--dry-run`
  parse input and resolve the CUDA range without touching tmux tabs or starting jobs

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

Exit mapping:

- `0` => Done
- `130` => Cancelled
- any other non-zero => Failed
- missing window and missing `.exit` => Cancelled

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
