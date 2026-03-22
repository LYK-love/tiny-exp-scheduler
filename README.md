[中文](README.md) | [English](README.en.md)

# tiny-exp-scheduler

`tiny-exp-scheduler` 是一个基于 [*tmux*](https://github.com/tmux/tmux/wiki) 的单机多卡 GPU 任务调度器。

它只做一件事：给定一组 shell 命令，选择本次运行允许使用的 GPU，在不同的 *tmux* 标签页中并行启动这些命令，并记录日志和退出状态。

它不负责生成命令，不提供领域特定语言（DSL, domain-specific language），也不处理分布式系统。一个任务（job）就是输入中的一个非空、非注释行；每个任务都必须是一条完整的 shell 命令。

默认情况下，一个运行中的任务会占用一张 GPU。也就是说，这个工具的常规运行模型是“一任务一 GPU”，因此它主要适用于单卡实验。对于少数根本不需要 CUDA 分配的任务，也支持 `none` 模式。

## 安装

依赖：

- Rust 1.74+
- `tmux`
- `nvidia-smi`

在仓库根目录安装：

```bash
git clone git@github.com:LYK-love/tiny-exp-scheduler.git
cd tiny-exp-scheduler
cargo install --path .
```

## 快速开始

先启动一个 `tmux` 会话：

```bash
tmux new-session -s exp
```

准备 `commands.txt`：

```text
python train.py --exp exp_a
python train.py --exp exp_b
```

运行调度器：

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
```

## 用法

必须在一个已有的 *tmux 会话*（tmux session）中运行。

```bash
tiny-exp-scheduler run [COMMANDS_FILE] [OPTIONS]
# 或：
cat commands.txt | tiny-exp-scheduler run [OPTIONS]
```

如果省略 `COMMANDS_FILE`，调度器会从标准输入读取命令。

常用选项：

- `--cuda-devices auto`
- `--cuda-devices none`
- `--cuda-devices 0,2,5`
- `--idle-memory-threshold-mb N`
- `--logs-dir DIR`
- `--keep-job-tabs`
- `--tick-seconds N`
- `--dry-run`

关于运行时语义和状态转换，请见[设计文档](docs/design.md)。如果你想看有代表性的实际用法，请见 [Workflow 示例](docs/workflows.md)。

## 命令文件

命令文件使用 shell 语法，但它本身不是一个要直接执行的 shell 脚本。

输入规则：

- 忽略空行
- 忽略以 `#` 开头的行
- 每个其余行都是一个任务（job）

惯例上把它命名为 `commands.txt`，是为了强调它是调度器的输入，而不是一个拿来直接 `bash xxx.sh` 的脚本。

## 推荐模式

推荐把输入文件中的每一行都写成“一条 shell 命令调用一个脚本”。

共享的环境准备逻辑放在包装脚本（wrapper script）里。`CUDA_VISIBLE_DEVICES` 由调度器从外部设置，不要在包装脚本里自行指定。

如果使用 `--cuda-devices none`，调度器就不会设置 `CUDA_VISIBLE_DEVICES`。

示例 `commands.txt`：

```text
bash scripts/train_one.sh exp_a configs/pong_a.yaml
bash scripts/train_one.sh exp_b configs/pong_b.yaml
bash scripts/train_one.sh exp_c configs/pong_c.yaml
bash scripts/train_one.sh exp_d configs/pong_d.yaml
```

示例包装脚本：

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

## 输出

默认输出目录：

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

调度器会在当前 *tmux* 会话中保留一个名为 `__sched__` 的汇总标签页。

## 更多内容

- [Workflow 示例](docs/workflows.md)
- [设计文档](docs/design.md)
- [examples/torch_hold_gpu.py](examples/torch_hold_gpu.py)
- [examples/torch-two-gpu-jobs.txt](examples/torch-two-gpu-jobs.txt)
- [examples/torch-four-gpu-jobs.txt](examples/torch-four-gpu-jobs.txt)

## 测试

```bash
cargo test
bash scripts/tmux-smoke.sh
```

## License

MIT
