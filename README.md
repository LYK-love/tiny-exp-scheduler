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
tiny-exp-scheduler run commands.txt --cuda-devices auto --verbose
```

## Command Format

The input file is not a shell script. The scheduler treats each non-empty, non-comment line as one job:

```text
# ignored
python train.py --exp exp_a
python train.py --exp exp_b
```

## GPU Selection
=======
If `COMMANDS_FILE` is omitted, the scheduler reads from standard input.

Common options:

- `--cuda-devices auto`
- `--cuda-devices none`
- `--cuda-devices 0,2,5`
- `--idle-memory-threshold-mb N`
- `--idle-utilization-threshold N`
- `--scheduler-name NAME`
- `--cpu-threads N`
- `--cpu-cores ARG`
- `--cpus-per-job N`
- `--logs-dir DIR`
- `--keep-job-tabs`
- `--verbose`

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

## Multiple Schedulers In One tmux Session

By default, the first scheduler in a tmux session uses `s` with job tabs named `j1`, `j2`, and so on. Additional schedulers automatically get names such as `s2` with job tabs like `s2j1`.

Long explicit scheduler names are compacted in tmux tab names, for example `s.twister-a1b2c3` and `twister-a1b2c3.j1`.

For stable names, pass an explicit namespace:

```bash
tiny-exp-scheduler run commands-a.txt --scheduler-name train-a --cuda-devices 0
tiny-exp-scheduler run commands-b.txt --scheduler-name train-b --cuda-devices 1
```

Those runs use scheduler tabs `s.train-a` and `s.train-b`, with job tabs like `train-a.j1`.

## CPU Thread Limits

Use `--cpu-cores` and `--cpus-per-job` when running several CPU-hungry GPU jobs at once:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices 0,1,2,3 \
  --cpu-cores 0-31 \
  --cpus-per-job 8
```

This splits the CPU pool into exclusive slots (`0-7`, `8-15`, `16-23`, `24-31`) and launches each job with `taskset`, so child processes such as DataLoader workers inherit the same CPU affinity.

If `--cpu-threads` is omitted, the thread limit defaults to `--cpus-per-job`. You can override it:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices 0,1,2,3 \
  --cpu-cores 0-31 \
  --cpus-per-job 8 \
  --cpu-threads 4
```

The scheduler sets common OpenMP/BLAS thread variables plus `DIAMOND_TORCH_NUM_THREADS` for each job.

See [CPU resource scheduling](docs/cpu.md) for machine inspection commands, slot allocation details, and tuning guidance.

## Logs

Default output:

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```


## Docs

- [CLI reference](docs/cli.md)
- [CPU resource scheduling](docs/cpu.md)
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
