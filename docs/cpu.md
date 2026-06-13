# CPU Resource Scheduling

`tiny-exp-scheduler` can allocate CPU resources in addition to GPUs. This is useful when each training job can otherwise use all CPU cores and create many worker processes or compute threads.

Typical sources of CPU oversubscription:

- one main training process per job
- PyTorch DataLoader worker processes
- OpenMP, MKL, OpenBLAS, NumExpr, or PyTorch CPU threads inside each process

## Query The Machine

Check how many logical CPU cores are visible:

```bash
nproc
```

Show CPU topology:

```bash
lscpu
```

Useful fields:

```text
CPU(s)
On-line CPU(s) list
Thread(s) per core
Core(s) per socket
Socket(s)
NUMA node(s)
NUMA node0 CPU(s)
NUMA node1 CPU(s)
```

Check how many logical cores the current process is allowed to use:

```bash
python -c "import os; print(len(os.sched_getaffinity(0))); print(sorted(os.sched_getaffinity(0)))"
```

This is the most relevant number when a server, container, Slurm job, or parent process already restricts CPU affinity.

## Core Slots

CPU affinity is enabled with:

```bash
--cpu-cores ARG
--cpus-per-job N
```

`--cpu-cores` chooses the CPU pool:

- `none`: no CPU affinity allocation. This is the default.
- `auto`: use all logical CPU cores visible to the scheduler process.
- `0-31` or `0-15,32-47`: use an explicit list/range of logical CPU IDs.

`--cpus-per-job` splits the CPU pool into fixed-size slots.

Example:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices 0,1,2,3 \
  --cpu-cores 0-31 \
  --cpus-per-job 8
```

This creates four slots:

```text
0-7
8-15
16-23
24-31
```

Each running job gets one slot. The scheduler starts the job through:

```bash
taskset -c <slot> bash -lc '<raw command>'
```

Child processes inherit this affinity, including DataLoader workers.

If the CPU pool is not divisible by `--cpus-per-job`, leftover cores are not assigned. For example, 70 logical cores with `--cpus-per-job 8` creates eight full slots and leaves six cores unused.

## Thread Limits

CPU affinity controls which cores a job may run on. Thread limits control how many compute threads each process should create.

Use:

```bash
--cpu-threads N
```

The scheduler sets:

```text
OMP_NUM_THREADS=N
MKL_NUM_THREADS=N
OPENBLAS_NUM_THREADS=N
NUMEXPR_NUM_THREADS=N
VECLIB_MAXIMUM_THREADS=N
BLIS_NUM_THREADS=N
DIAMOND_TORCH_NUM_THREADS=N
DIAMOND_TORCH_INTEROP_THREADS=1
```

If `--cpu-cores` is enabled and `--cpu-threads` is omitted, the scheduler uses `--cpus-per-job` as the thread limit.

Example:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices 0,1,2,3 \
  --cpu-cores auto \
  --cpus-per-job 8 \
  --cpu-threads 2
```

This gives each job 8 logical cores but asks compute libraries to use at most 2 threads per process.

## Best Practice: 8 GPUs, 8 Independent Training Jobs

This is the target scenario:

- 8 GPUs
- 8 commands in the scheduler input file
- each command is an independent training job
- each job should use one GPU
- each job should get one CPU slot

First, check how many logical CPU cores are visible:

```bash
nproc
```

For an 80-logical-core machine, a clean starting point is one CPU slot per GPU:

```text
cpus_per_job = floor(80 / 8) = 10
```

Run:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices 0,1,2,3,4,5,6,7 \
  --cpu-cores auto \
  --cpus-per-job 10 \
  --cpu-threads 2 \
  --verbose \
  --keep-job-tabs
```

This creates 8 CPU slots, each with 10 logical cores. With 80 visible logical cores, the slots are:

```text
0-9
10-19
20-29
30-39
40-49
50-59
60-69
70-79
```

Each training job is launched with one slot through `taskset`, so its main process and DataLoader worker processes inherit the same CPU affinity.

Use `--cpu-threads 2` as the default starting point. It limits each process's OpenMP/MKL/OpenBLAS/PyTorch compute threads while still allowing some CPU parallelism inside each training process.

If the jobs use many DataLoader workers, prefer:

```bash
--cpu-threads 1
```

If the machine has a different number of logical cores, adjust only `--cpus-per-job`:

```text
cpus_per_job = floor(logical_cpu_cores / 8)
```

Examples:

```text
64 logical cores -> --cpus-per-job 8
80 logical cores -> --cpus-per-job 10
96 logical cores -> --cpus-per-job 12
128 logical cores -> --cpus-per-job 16
```

If you want to leave CPU cores for the OS or other users, use a smaller value. For example, on an 80-logical-core machine:

```bash
--cpu-cores auto --cpus-per-job 8 --cpu-threads 2
```

This uses at most 64 logical cores for the 8 training jobs and leaves the remaining cores unassigned by the scheduler.

## Practical Checks

Watch GPU utilization:

```bash
nvidia-smi dmon -s pucm
```

Watch CPU use and load:

```bash
htop
```

Check scheduler dry-run output:

```bash
tiny-exp-scheduler run commands.txt \
  --cuda-devices none \
  --cpu-cores auto \
  --cpus-per-job 8 \
  --dry-run
```

The dry run prints the resolved CPU slots without touching `tmux`.

## Notes

- `taskset` is required when `--cpu-cores` is enabled.
- CPU IDs are logical CPU IDs, not physical core IDs.
- `--cpu-cores auto` is portable across machines, but it may use every core visible to the process.
- Use an explicit range when you want to leave cores for the OS or other users.
- DataLoader worker count is controlled by the training program or config, not by `--cpu-threads`.
