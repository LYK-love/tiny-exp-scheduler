# Workflow 示例

如果你还没有安装或使用过这个工具，请先看 [README.md](../README.md)。如果你想了解这些示例背后的运行模型和状态规则，请看 [design.md](design.md)。

## 前提

```bash
cd tiny-exp-scheduler
tmux new-session -s exp
```

如果要跑 GPU 示例，还需要：

- 已安装 `torch`
- `torch.cuda.is_available()` 为真
- 机器上有可用 GPU

## 1. 最小队列

命令：

```bash
tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices 0
```

含义：

- 只允许使用一张 GPU
- 同一时间只会运行一个任务
- `job_2` 需要等待 `job_1`

检查：

```bash
cat logs/job_1.exit
cat logs/job_2.exit
```

两者都应为 `0`。

## 2. 多 GPU 并发

命令：

```bash
tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

含义：

- 允许使用四张 GPU
- 四个任务可以并行启动
- 每个运行中的任务都有自己的 tmux 标签页

## 3. 每行调用一个脚本

这是最常见的真实实验模式：共享环境写进一个脚本，`commands.txt` 只保留每次运行不同的参数。

示例 `commands.txt`：

```text
bash scripts/train_one.sh exp_a configs/pong_a.yaml
bash scripts/train_one.sh exp_b configs/pong_b.yaml
bash scripts/train_one.sh exp_c configs/pong_c.yaml
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

命令：

```bash
tiny-exp-scheduler run commands.txt --cuda-devices auto
```

含义：

- 输入文件中的每一行仍然是一条 shell 命令
- 共享环境保留在包装脚本中
- `CUDA_VISIBLE_DEVICES` 由调度器设置，而不是由脚本内部设置

## 4. GPU 占用示例

单 GPU 排队：

```bash
tiny-exp-scheduler run examples/torch-two-gpu-jobs.txt --cuda-devices 0
```

预期：

- `job_1` 先占用 GPU
- `job_2` 等待
- `nvidia-smi` 能看到额外的显存占用

四 GPU 并发：

```bash
tiny-exp-scheduler run examples/torch-four-gpu-jobs.txt --cuda-devices 0,1,2,3
```

预期：

- `job_1` 到 `job_4` 同时启动
- 每个任务绑定一张 GPU

## 5. Dry Run

命令：

```bash
tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices auto --dry-run
```

含义：

- 解析输入
- 解析最终 CUDA 范围
- 打印执行计划
- 不改当前 tmux 标签页
- 不启动任何任务

## 6. 中断一个任务

命令：

```bash
tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

操作：

- 进入某个运行中的任务标签页
- 按 `Ctrl+C`

预期：

- 该任务进入 `Cancelled`
- 对应 `.exit` 文件变成 `130`
- 对应 GPU 被释放
- 其他任务继续运行

另一种操作：

```bash
tmux kill-window -t exp:job_3
```

这也会取消该任务，并释放对应 GPU。

本项目由 AI 和人类共同编写。
