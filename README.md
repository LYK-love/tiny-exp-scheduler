[中文](README.md) | [English](README.en.md)

# tiny-exp-scheduler

`tiny-exp-scheduler` 是一个基于 [*tmux*](#术语说明) 的 GPU 任务（job）调度器，适用于单机多卡实验场景。

它接收一组 shell 命令，自动在可用 GPU 中分配资源，把各条命令并行运行在不同的 [*tmux 标签页*](#术语说明) 中，并记录日志与退出状态。

它不负责生成命令，不提供领域特定语言（DSL, domain-specific language），也不处理分布式系统。输入中的每一行都只是一个 shell 命令；调度器负责的范围仅限于 GPU 分配、*tmux* 启动以及任务（job）状态跟踪。

## 执行模型

```text
commands.txt
    |
    v
__scheduler__ tab
    |
    +--> job_1 tab  (GPU a)
    +--> job_2 tab  (GPU b)
    +--> job_3 tab  (GPU c)
    +--> ...
```

本次运行允许使用的 GPU 集合（allowed GPU set）会在启动时确定一次，并在整个运行过程中保持不变。

## 调度逻辑

```text
read jobs
select allowed GPUs
while unfinished jobs exist:
    wait for an allowed GPU to become free
    launch next job in a new tmux tab
    set CUDA_VISIBLE_DEVICES for that job
    record log and exit status
print final summary
```

## 安装

依赖：

- Rust 1.74+
- `tmux`
- NVIDIA System Management Interface（`nvidia-smi`）

构建：

```bash
git clone git@github.com:LYK-love/tiny-exp-scheduler.git
cd tiny-exp-scheduler
cargo build --release
```

二进制文件：

```bash
target/release/tiny-exp-scheduler
```

## 使用方法

运行前，你必须已经进入一个 *tmux 会话*（tmux session），并位于其中一个 *tmux 标签页*（tmux tab）中。

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
tiny-exp-scheduler run commands.txt --cuda-devices 0,2,5
tiny-exp-scheduler run commands.txt --logs-dir logs
tiny-exp-scheduler run commands.txt --dry-run
cat commands.txt | tiny-exp-scheduler run --cuda-devices auto
```

启动时会发生这些事情：

- 当前 *tmux 标签页* 会被重命名为 `__scheduler__`
- 各个任务标签页会在它后面依次创建
- 如果当前 *tmux 会话* 中已经存在 `__scheduler__`，命令会直接失败

运行结束后：

- `__scheduler__` 标签页会保留
- 调度器会在该标签页中打印最终汇总信息

## GPU 选择

- `--cuda-devices auto`  
  启动时检查所有 GPU，把当时空闲的 GPU 作为本次运行的允许 GPU 集合（allowed GPU set）。

- `--cuda-devices 0,2,5`  
  只使用列出的 GPU。如果其中任意一张卡在启动时处于忙碌状态，命令会直接失败。

空闲判定规则：

```text
memory.used <= threshold
and
utilization.gpu == 0
```

默认阈值为 `64` 兆字节（MB）。

可以通过下面的参数调整：

```bash
--idle-memory-threshold-mb N
```

启动时，调度器会打印最终采用的 GPU 集合，例如：

```text
Final CUDA device range: cuda:0,cuda:2,cuda:5
```

- `--dry-run`  
  只读取输入、检查 GPU，并打印执行计划；不会改动 *tmux* 标签页，也不会启动任何任务（job）。

## 输入规则

- 遵循 shell 语法
- 忽略空行，以及以 `#` 开头的行，也就是注释
- 每个剩余行都被视为一个任务（job）

示例：

```text
# commands.txt
python train.py --exp exp_a
python train.py --exp exp_b
python train.py --exp exp_c
```

## 脚本模式（Wrapper Script）

推荐的使用模式是：让输入文件中的每一行都保持为一条完整的 shell 命令，而这条命令本身去调用一个脚本文件。

把共享的环境准备逻辑放进一个包装脚本（wrapper script）中。不要在这个脚本里设置 `CUDA_VISIBLE_DEVICES`；这个变量应当由调度器从外部注入。

针对每次运行不同的参数，可以从 `commands.txt` 传给脚本，例如通过 `$1`、`$2` 等位置参数。

示例 `commands.txt`：

```text
bash scripts/run_experiment.sh exp_a
bash scripts/run_experiment.sh exp_b
bash scripts/run_experiment.sh exp_c
bash scripts/run_experiment.sh exp_d
```

示例包装脚本（wrapper script）：

```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

export PYTHONPATH="${PROJECT_ROOT}/src"
export DATASET_PATH="${PROJECT_ROOT}/dataset/pong/test"
export BACKEND_ENDPOINT="http://localhost:8080"

python "${PROJECT_ROOT}/src/tools/run_experiment.py" \
  --dataset-path "${DATASET_PATH}" \
  --backend-endpoint "${BACKEND_ENDPOINT}" \
  --run-name "$1"
```

`CUDA_VISIBLE_DEVICES` 由调度器设置，并会被任务进程（job process）继承，因此不应在包装脚本中再次指定。

## 最小示例

先启动 *tmux*：

```bash
tmux new-session -s exp
```

然后运行：

```bash
tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices 0
```

## 日志与状态

默认输出目录：

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

任务状态（job state）由退出状态码（exit status）推导得到：

- `0` -> `Done`
- `130` -> `Cancelled`
- 其他非零 -> `Failed`

如果某个任务标签页消失，并且没有找到对应的 `.exit` 文件，调度器会将该任务标记为 `Cancelled`。

运行结束后，`__scheduler__` 标签页会打印：

- 最终采用的 CUDA 设备范围
- 日志目录
- 任务总数
- `Done` / `Failed` / `Cancelled` 的数量
- `Failed` / `Cancelled` 的任务编号

## 更多内容

- [设计文档](docs/design.md)
- [Workflow 示例](docs/workflows.md)
- [examples/torch_hold_gpu.py](examples/torch_hold_gpu.py)
- [examples/torch-two-gpu-jobs.txt](examples/torch-two-gpu-jobs.txt)
- [examples/torch-four-gpu-jobs.txt](examples/torch-four-gpu-jobs.txt)

## 测试

```bash
cargo test
bash scripts/tmux-smoke.sh
```

## 术语说明

- [*tmux*](https://github.com/tmux/tmux/wiki)：终端复用器（terminal multiplexer）。
- 任务（job）：输入文件中一个非空且非注释的行；每个任务都是一条完整的 shell 命令。
- *tmux 会话*（tmux session）：调度器当前运行所在的 *tmux* 会话。
- *tmux 标签页*（tmux tab）：本文中对 *tmux window* 的称呼。
- 允许 GPU 集合（allowed GPU set）：本次运行允许使用的 GPU 集合；它在启动时确定一次。

## License

MIT
