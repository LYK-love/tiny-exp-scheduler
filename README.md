# tiny-exp-scheduler

Minimal `tmux` scheduler for running one-line experiment commands on selected CUDA GPUs.

It reads a plain text command list, starts jobs in separate `tmux` windows, assigns one GPU per running job by setting `CUDA_VISIBLE_DEVICES`, and writes scheduler-managed logs plus exit codes.

## Install

Requirements:

- Rust 1.74+
- `tmux`
- `nvidia-smi`

```bash
git clone https://github.com/LYK-love/tiny-exp-scheduler.git
cd tiny-exp-scheduler
cargo install --path .
```

## Quick Start

Run inside an existing `tmux` session:

```bash
tmux new-session -s exp
```

Create `commands.txt`:

```text
python train.py --exp exp_a
python train.py --exp exp_b
```

Start the scheduler:

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
```

## Command Format

The input file is not a shell script. The scheduler treats each non-empty, non-comment line as one job:

```text
# ignored
python train.py --exp exp_a
python train.py --exp exp_b
```

## GPU Selection

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
tiny-exp-scheduler run commands.txt --cuda-devices 0,2,5
tiny-exp-scheduler run commands.txt --cuda-devices none
```

- `auto`: detect idle GPUs once at startup.
- `0,2,5`: use exactly these GPUs; each must pass the idle check.
- `none`: do not set `CUDA_VISIBLE_DEVICES`.

Idle means both checks pass:

```text
memory.used <= --idle-memory-threshold-mb
utilization.gpu <= --idle-utilization-threshold
```

Defaults are strict: `64 MiB` and `0%`.

If a GPU has expected background memory or light utilization, relax the startup check:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices 0 \
  --idle-memory-threshold-mb 5000 \
  --idle-utilization-threshold 40
```

These options only decide whether a GPU is accepted as available. They do not free VRAM or stop other processes.

## Logs

Default output:

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

Useful options:

```bash
--logs-dir DIR
--keep-job-tabs
--verbose
--dry-run
```

## Docs

- [CLI reference](docs/cli.md)
- [workflow demos](docs/workflows.md)
- [design notes](docs/design.md)
- [examples](examples)

## Test

```bash
cargo test
bash scripts/tmux-smoke.sh
```

## License

MIT
