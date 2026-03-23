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
utilization.gpu == 0
```

Default:

```text
64
```

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

### `--keep-job-tabs`

Keep finished job tabs visible in `tmux`.

Default behavior:

- a finished job tab exits and disappears

With `--keep-job-tabs`:

- the tab remains visible after the command exits

### `--verbose`

Print per-job runtime events in the `__sched__` tab.

This is mainly useful when you want to watch jobs finish in real time.

Example output:

```text
[FINISHED] job_3 -> Done (exit=0)
[FINISHED] job_4 -> Failed (exit=1)
[FINISHED] job_2 -> Cancelled (exit=130)
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
