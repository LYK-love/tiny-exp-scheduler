# 设计文档

## 一句话

`tiny-exp-scheduler` 把“逐行 shell 命令”映射为“当前 tmux session 中的一组独立 job tab”，并在一个冻结的 CUDA device 范围内做简单轮询调度。

## 系统总览

```text
commands.txt
    |
    v
current tmux tab
    |
    +--> __scheduler__
    +--> job_1
    +--> job_2
    +--> ...
```

启动路径：

```text
tiny-exp-scheduler run ...
    |
    +--> check tmux context
    +--> resolve CUDA device range
    +--> rename current tab -> __scheduler__
    +--> enter polling scheduler loop
```

## 核心原则

- 输入即命令
- 不解析命令语义
- 不生成命令
- 只调度命令
- 调度器保持简单

## 输入模型

输入来源：

- 文件：`tiny-exp-scheduler run commands.txt`
- 标准输入：`cat commands.txt | tiny-exp-scheduler run`

规则：

- 忽略空行
- 忽略以 `#` 开头的行
- 每个剩余行就是一个 job

## tmux 模型

约束：

- 用户必须先进入一个已有 tmux session
- 程序不会创建新的 session
- 当前 tab 会被重命名为 `__scheduler__`
- 每个 job 一个新 tab，名称为 `job_X`
- `__scheduler__` 默认保留
- 同一个 session 中只允许一个 `__scheduler__`

## GPU 模型

只使用一个选项：

```text
--cuda-devices auto
--cuda-devices 0,2,5
```

`auto`：

- 启动时通过 `nvidia-smi` 找出当前空闲 GPU
- 冻结为本次运行的资源池
- 后续即使别的 GPU 变空闲，也不会被加入

显式列表：

- 只允许使用用户指定的 GPU id
- 程序会检查这些 GPU 当前都空闲
- 只要有一个忙，就直接失败
- 不会偷偷缩成更小的范围

当前空闲判定：

```text
memory.used <= threshold
utilization.gpu == 0
```

其中 `threshold` 由 `--idle-memory-threshold-mb` 控制，默认 `64`。

冻结语义：

```text
startup:
  --cuda-devices auto
  idle GPUs found -> [0,2,5]

runtime:
  scheduler only allocates from [0,2,5]
  GPU 3 becoming idle later does not change the pool
```

## Job 状态机

```text
Pending
  -> Scheduled
  -> Running
  -> Done / Failed / Cancelled
```

语义：

- `Pending`：还没拿到 GPU
- `Scheduled`：已分配 GPU，待启动
- `Running`：tmux job tab 已创建
- `Done`：退出码 `0`
- `Failed`：非零且非 `130`
- `Cancelled`：退出码 `130`，或 window 消失且无 `.exit`

## 调度循环

伪代码：

```text
loop:
  schedule pending jobs onto frozen CUDA pool
  start scheduled jobs in tmux tabs
  finalize running jobs whose tabs disappeared
  sleep(tick_seconds)
```

顺序固定：

```text
1. Pending -> Scheduled
2. Scheduled -> Running
3. Running -> Finished
```

## 执行模型

每个 job 都被物化成一段显式 shell 脚本：

```bash
CUDA_VISIBLE_DEVICES=<gpu_id> \
PYTHONUNBUFFERED=1 \
<raw command> \
2>&1 | tee logs/job_X.log
```

脚本还会：

- 打印 job 元信息
- 开启 `set -o pipefail`
- 写 `logs/job_X.exit`

## 日志与退出码

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

映射：

- `0` -> `Done`
- `130` -> `Cancelled`
- 其他非零 -> `Failed`
- 无 `.exit` 且 window 消失 -> `Cancelled`

## 边界情况

`Ctrl+C`：

- 通常写出 `130`
- job 进入 `Cancelled`
- 对应 GPU 释放

`kill-window`：

- window 消失
- 若无 `.exit`，视为 `Cancelled`
- GPU 释放

`kill-session`：

- 所有运行中 job 都会被视为结束
- 缺少 `.exit` 的 job 记为 `Cancelled`
- GPU 释放

## 当前 CLI

```bash
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices auto] [--tick-seconds N]
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices 0,2,5] [--tick-seconds N]
tiny-exp-scheduler run [commands.txt] [--dry-run]
```

补充：

- `--dry-run` 只解析输入并解析最终 CUDA 范围，不改 tmux tab
- scheduler 汇总会打印最终 CUDA 范围和 logs 目录

本项目由 AI 和人类共同编写。
