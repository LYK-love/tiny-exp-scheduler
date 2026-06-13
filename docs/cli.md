# CLI Reference

Start with [README.md](../README.md) for installation and basic usage. See
[design.md](design.md) for runtime semantics and [workflows.md](workflows.md) for representative command patterns.

## Command Surface

```bash
tiny-exp-scheduler run [COMMANDS_FILE] [OPTIONS]
cat commands.txt | tiny-exp-scheduler run [OPTIONS]
tiny-exp-scheduler -h | --help
```

`COMMANDS_FILE` is optional. If omitted, the scheduler reads from standard input.

## Options

### `--cuda-devices ARG`

Selects the GPU mode for this run.

Supported values:

- `auto`
  Detect idle GPUs once at startup and freeze that set.
- `none`
  Do not allocate GPUs and do not set `CUDA_VISIBLE_DEVICES`.
- `0,2,5`
  Use exactly these GPUs. All listed GPUs must already be idle.

Default:

```text
auto
```

### `--idle-memory-threshold-mb N`

Used only when GPU idleness is checked.

The current idle rule is:

```text
memory.used <= N
utilization.gpu <= idle utilization threshold
```

Default:

```text
64
```

### `--idle-utilization-threshold N`

Used only when GPU idleness is checked.

The current idle rule is:

```text
memory.used <= idle memory threshold
utilization.gpu <= N
```

Default:

```text
0
```

Example:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices 0 \
  --idle-memory-threshold-mb 5000 \
  --idle-utilization-threshold 40
```

This accepts GPU 0 only if both conditions are true:

- `memory.used <= 5000`
- `utilization.gpu <= 40`

### `--logs-dir DIR`

Directory for scheduler-managed log and exit files.

Default:

```text
logs
```

Typical output shape:

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

### `--scheduler-name NAME`

Use a stable tmux namespace for this scheduler run.

Rules:

- `NAME` may contain ASCII letters, digits, `_`, `-`, and `.`
- scheduler tab: `s.NAME` for short names, or `s.PREFIX-HASH` for long names
- job tabs: `NAME.j1`, `NAME.j2`, ... for short names, or `PREFIX-HASH.j1`, `PREFIX-HASH.j2`, ... for long names

If omitted, the scheduler chooses the first available namespace in the current tmux session. The first default run uses `s` and `j1`; later concurrent runs use names such as `s2` and `s2j1`.

### `--cpu-threads N`

Limit CPU compute threads per running job.

See [CPU resource scheduling](cpu.md) for machine inspection commands and tuning guidance.

The scheduler sets these environment variables for each job:

- `OMP_NUM_THREADS`
- `MKL_NUM_THREADS`
- `OPENBLAS_NUM_THREADS`
- `NUMEXPR_NUM_THREADS`
- `VECLIB_MAXIMUM_THREADS`
- `BLIS_NUM_THREADS`
- `DIAMOND_TORCH_NUM_THREADS`
- `DIAMOND_TORCH_INTEROP_THREADS=1`

This is useful when several GPU jobs run concurrently and each job otherwise oversubscribes CPU cores.

If CPU affinity is also enabled with `--cpu-cores` and `--cpus-per-job`, this value defaults to `--cpus-per-job` when `--cpu-threads` is omitted.

### `--cpu-cores ARG`

Select the CPU core pool for scheduler-managed affinity.

See [CPU resource scheduling](cpu.md) for examples and CPU topology commands.

Supported values:

- `none`
  Do not allocate CPU core slots. This is the default.
- `auto`
  Use all logical CPU cores reported by the OS.
- `0-15,32-47`
  Use exactly these logical CPU cores. Comma-separated core IDs and inclusive ranges are both supported.

When this option is not `none`, `--cpus-per-job` is required.

### `--cpus-per-job N`

Split the CPU core pool into fixed-size slots and allocate one slot per running job.

Example:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices 0,1,2,3 \
  --cpu-cores 0-31 \
  --cpus-per-job 8
```

This creates four CPU slots:

```text
0,1,2,3,4,5,6,7
8,9,10,11,12,13,14,15
16,17,18,19,20,21,22,23
24,25,26,27,28,29,30,31
```

Each job is launched through `taskset -c <slot> bash -lc '<command>'`, so subprocesses inherit the affinity.

### `--keep-job-tabs`

Keep finished job tabs visible in `tmux`.

Default behavior:

- a finished job tab exits and disappears

With `--keep-job-tabs`:

- the tab remains visible after the command exits

### `--verbose`

Print per-job runtime events in the scheduler tab.

This is mainly useful when you want to watch jobs finish in real time.

Example output:

```text
[FINISHED] j3 -> Done (exit=0)
[FINISHED] j4 -> Failed (exit=1)
[FINISHED] j2 -> Cancelled (exit=130)
```

### `--tick-seconds N`

Polling interval for the scheduler loop.

Default:

```text
1
```

### `--dry-run`

Resolve the command input and CUDA device range without touching `tmux` or launching any jobs.

## Typical Forms

Single-GPU queue:

```bash
tiny-exp-scheduler run commands.txt --cuda-devices 0
```

Multi-GPU run:

```bash
tiny-exp-scheduler run commands.txt --cuda-devices 0,1,2,3
```

Auto-detect idle GPUs:

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
```

No CUDA allocation:

```bash
tiny-exp-scheduler run commands.txt --cuda-devices none
```

Verbose runtime view:

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto --verbose
```
