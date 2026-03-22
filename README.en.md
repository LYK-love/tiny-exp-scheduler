[中文](README.md) | [English](README.en.md)

# tiny-exp-scheduler

`tiny-exp-scheduler` is a minimal [*tmux*](https://github.com/tmux/tmux/wiki)-based GPU job scheduler for a single multi-GPU machine.

It does one thing: given a list of shell commands, it assigns available GPUs, launches the commands in parallel in separate *tmux* tabs, and records logs and exit status.

It does not generate commands, define a *domain-specific language (DSL)*, or manage distributed systems. A *job* is one non-empty, non-comment input line, and each *job* is one complete shell command.

By default, one running job occupies one GPU. In other words, the normal execution model is one
job per GPU, so this tool is mainly meant for single-GPU experiments. A `none` mode is also
available for the rarer case where jobs should not receive any CUDA allocation from the scheduler.

## Installation

Requirements:

- Rust 1.74+
- `tmux`
- `nvidia-smi`

Install from the repository root:

```bash
git clone git@github.com:LYK-love/tiny-exp-scheduler.git
cd tiny-exp-scheduler
cargo install --path .
```

## Quick Start

Start a `tmux` session:

```bash
tmux new-session -s exp
```

Prepare `commands.txt`:

```text
python train.py --exp exp_a
python train.py --exp exp_b
```

Run the scheduler:

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
```

## Usage

Run inside an existing *tmux* session.

```bash
tiny-exp-scheduler run [COMMANDS_FILE] [OPTIONS]
# Or:
cat commands.txt | tiny-exp-scheduler run [OPTIONS]
```

If `COMMANDS_FILE` is omitted, the scheduler reads from standard input.

Common options:

- `--cuda-devices auto`
- `--cuda-devices none`
- `--cuda-devices 0,2,5`
- `--idle-memory-threshold-mb N`
- `--logs-dir DIR`
- `--keep-job-tabs`
- `--tick-seconds N`
- `--dry-run`

For runtime semantics and state transitions, see the [design document](docs/design.en.md). For
representative end-to-end usage patterns, see [workflow demos](docs/workflows.en.md).

## Command File

The command file uses shell syntax. Therefore, this project does not define a DSL.

The file is parsed as a list of shell commands, not executed as a shell script.

Input rules:

- ignore empty lines
- ignore lines starting with `#`
- treat each remaining line as one *job*

The conventional name `commands.txt` is intentional. The file is meant to be read by the scheduler, not run directly as a `.sh` script.

## Recommended Pattern

Keep each input line as one shell command, and let that command invoke one script.

Put shared environment setup in a wrapper script. Let the scheduler set `CUDA_VISIBLE_DEVICES` from the outside.

If you use `--cuda-devices none`, the scheduler does not set `CUDA_VISIBLE_DEVICES` at all.

Example `commands.txt`:

```text
bash scripts/train_one.sh exp_a configs/pong_a.yaml
bash scripts/train_one.sh exp_b configs/pong_b.yaml
bash scripts/train_one.sh exp_c configs/pong_c.yaml
bash scripts/train_one.sh exp_d configs/pong_d.yaml
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

## Output

Default output:

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

The scheduler keeps a summary tab named `__sched__` in the current *tmux* session.

## More

- [workflow demos](docs/workflows.en.md)
- [design document](docs/design.en.md)
- [examples/torch_hold_gpu.py](examples/torch_hold_gpu.py)
- [examples/torch-two-gpu-jobs.txt](examples/torch-two-gpu-jobs.txt)
- [examples/torch-four-gpu-jobs.txt](examples/torch-four-gpu-jobs.txt)

## Tests

```bash
cargo test
bash scripts/tmux-smoke.sh
```

## License

MIT
