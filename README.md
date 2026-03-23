# tiny-exp-scheduler

> You can use AI to translate or explain this document and the rest of the project's documentation in your preferred language.
>
> 你可以使用 AI 将本文档和本项目的其他文档翻译成你偏好的语言，或为你解读其中的内容。

`tiny-exp-scheduler` is a minimal [*tmux*](https://github.com/tmux/tmux/wiki)-based scheduler for single-GPU deep learning jobs on a single multi-GPU machine.

Each experiment is a one-line shell command, called a *job*. The scheduler reads a list of jobs, assigns available GPUs, launches them in parallel in separate *tmux* windows, and records logs and exit status.

It introduces no extra abstractions and no domain-specific language (DSL): everything is plain shell commands and classic Linux tools. By default, each job occupies one GPU; a `none` mode is also available for jobs that should not receive any CUDA allocation.

> I'll consider supporting multi-GPU experiments in future :)

## Installation

Requirements:

- Rust 1.74+
- `tmux`
- `nvidia-smi`

Install from source:

```bash
git clone https://github.com/LYK-love/tiny-exp-scheduler.git
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

Run the scheduler inside an existing *tmux* session.

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
- `--verbose`
- `--tick-seconds N`
- `--dry-run`

For a full option reference, see [CLI reference](docs/cli.md). For runtime semantics and state transitions, see the [design document](docs/design.md). For representative end-to-end usage patterns, see [workflow demos](docs/workflows.md).

## Command File Format

The command file is conventionally named `commands.txt`. It uses shell-like command syntax。

> Remember that this project does not define a DSL. Everything here is just shell.

The `.txt` suffix is intentional: it emphasizes that the file is meant to be read by the scheduler, not executed directly as a `.sh` script.

The scheduler parses the file as a list of commands rather than running it as a shell script.

Input rules:

- ignore empty lines
- ignore lines starting with `#`
- treat each remaining line as one *job*

## Recommended Pattern

Keep each input line as one shell command, and let that command invoke one wrapper script.

Put shared environment setup in the wrapper script, for example environment activation, common exports, and the final command invocation. Let the scheduler set `CUDA_VISIBLE_DEVICES` from the outside.

If you use `--cuda-devices none`, the scheduler does not set `CUDA_VISIBLE_DEVICES` at all.

Example `commands.txt`:

```text
bash scripts/train_one.sh exp_a configs/pong_a.yaml data/pong
bash scripts/train_one.sh exp_b configs/pong_b.yaml data/pong
bash scripts/train_one.sh exp_c configs/pong_c.yaml data/pong
bash scripts/train_one.sh exp_d configs/pong_d.yaml data/pong
```

Example wrapper script:

```bash
#!/usr/bin/env bash
set -euo pipefail

: "${CONDA_ENV_NAME:=ml}"

if ! command -v conda >/dev/null 2>&1; then
  echo "[ERROR] conda was not found in PATH." >&2
  exit 1
fi

CONDA_BASE="$(conda info --base)"
# shellcheck source=/dev/null
source "${CONDA_BASE}/etc/profile.d/conda.sh"
conda activate "${CONDA_ENV_NAME}"

RUN_NAME="$1"
CONFIG_PATH="$2"
DATASET_PATH="$3"

python src/main.py \
  --config "${CONFIG_PATH}" \
  --dataset-path "${DATASET_PATH}" \
  --run-name "${RUN_NAME}"
```

## Output

Default scheduler output:

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

The scheduler keeps a summary window named `__sched__` in the current *tmux* session.

Jobs may still write their own logs, checkpoints, or output files elsewhere; those are not redirected into the scheduler log files. Scheduler logging only captures the command's stdout and stderr.

## More

- [CLI reference](docs/cli.md)
- [workflow demos](docs/workflows.md)
- [design document](docs/design.md)
- [examples](./examples)：
  * [examples/torch_hold_gpu.py](examples/torch_hold_gpu.py)
  * [examples/torch-two-gpu-jobs.txt](examples/torch-two-gpu-jobs.txt)
  * [examples/torch-four-gpu-jobs.txt](examples/torch-four-gpu-jobs.txt)

## Tests

```bash
cargo test

bash scripts/tmux-smoke.sh
```

## License

MIT
