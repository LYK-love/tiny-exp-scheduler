# 设计文档

## 项目定位

`tiny-exp-scheduler` 是一个给单机 GPU 实验用的小型调度器。它接收一组现成的 shell 命令，把当前 tmux 标签页（tab）变成调度器窗口，再在同一个 tmux 会话（session）里为每条命令新建一个任务标签页，并在一组预先确定的 GPU 中做简单分配。

## 系统总览

```text
commands.txt
    |
    v
tmux 会话（session）
    |
    +--> 当前标签页 -> __scheduler__
    +--> 后续新建标签页 -> job_1
    +--> 后续新建标签页 -> job_2
    +--> ...
```

程序启动后，大致会按这个顺序工作：

```text
tiny-exp-scheduler run ...
    |
    +--> 确认当前运行在 tmux 中
    +--> 读取命令列表
    +--> 确定这次允许运行任务的 GPU 列表
    +--> 把当前标签页改名为 __scheduler__
    +--> 进入轮询调度循环
```

## 设计原则

- 输入是什么命令，就执行什么命令
- 不尝试理解训练参数、模型名字、脚本含义
- 不帮用户生成命令，也不做参数展开
- 只解决“怎么排队、在哪张卡上跑、日志写到哪里”这几个问题
- 实现尽量简单，避免变成复杂编排系统

## 输入方式

输入来源：

- 文件：`tiny-exp-scheduler run commands.txt`
- 标准输入：`cat commands.txt | tiny-exp-scheduler run`

输入文件的规则很简单：

- 忽略空行
- 忽略以 `#` 开头的行
- 每个剩余行就是一个任务（job）

## tmux 运行方式

这个项目把 tmux 当成执行环境和观察界面，因此有这些明确约束：

- 用户必须先进入一个已有 tmux 会话（session），并位于其中一个标签页（tab）
- 程序不会创建新的会话
- 当前标签页会被重命名为 `__scheduler__`
- 每个任务对应一个新标签页，名称为 `job_X`
- `__scheduler__` 默认保留
- 同一个会话中只允许一个 `__scheduler__`

## GPU 选择与分配

命令行里只用一个选项控制 GPU 范围：

```text
--cuda-devices auto
--cuda-devices 0,2,5
```

如果使用 `auto`：

- 启动时通过 `nvidia-smi` 检查当前哪些 GPU 没在忙
- 把这些 GPU 记录为“本次运行允许使用的 GPU 列表”
- 后续即使别的 GPU 变空闲，也不会临时加入这次调度

如果使用显式列表：

- 只允许使用用户指定的 GPU 编号（device id）
- 程序会检查这些 GPU 当前都空闲
- 只要有一个忙，就直接失败
- 不会偷偷缩成更小的范围

当前判断 GPU 是否空闲，使用的是这两个条件：

```text
memory.used <= threshold
utilization.gpu == 0
```

其中 `threshold` 由 `--idle-memory-threshold-mb` 控制，默认值是 `64`。也就是说，默认情况下只有“显存占用不超过 64 MiB，且 GPU 利用率为 0”的卡才会被视为空闲。

为什么只在启动时确定一次 GPU 列表：

```text
启动时：
  --cuda-devices auto
  检查得到空闲 GPU -> [0,2,5]

运行中：
  调度器只会在 [0,2,5] 里分配任务
  后来 GPU 3 变空闲，也不会自动加入

这样做的目的，是让一次运行的资源边界保持稳定，不会因为机器上其他任务的变化而半途改变分配范围。
```

## 任务状态

```text
Pending
  -> Scheduled
  -> Running
  -> Done / Failed / Cancelled
```

各个状态的含义如下：

- `Pending`：还没拿到 GPU
- `Scheduled`：已分配 GPU，待启动
- `Running`：tmux 任务标签页已创建
- `Done`：退出状态码（exit status / exit code）为 `0`
- `Failed`：非零且非 `130`
- `Cancelled`：退出状态码为 `130`，或窗口（window）消失且无 `.exit`

## 调度循环

调度器每隔一段时间轮询一次，逻辑如下：

```text
loop:
  给还没启动的任务分配可用 GPU
  启动已经分配好 GPU 的任务
  检查哪些运行中的任务已经结束
  sleep(tick_seconds)
```

这三个步骤的顺序是固定的：

```text
1. Pending -> Scheduled
2. Scheduled -> Running
3. Running -> Finished
```

## 实际执行的命令

每个任务最终都会被展开成一段明确的 shell 脚本，大致形态如下：

```bash
CUDA_VISIBLE_DEVICES=<gpu_id> \
PYTHONUNBUFFERED=1 \
<raw command> \
2>&1 | tee logs/job_X.log
```

除了执行原始命令，它还会做这些事：

- 打印任务元信息
- 开启 `set -o pipefail`
- 写 `logs/job_X.exit`

## 日志与结束状态

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

任务结束后，程序会根据[退出状态码（exit status / exit code）](https://en.wikipedia.org/wiki/Exit_status)判断状态：

- `0` -> `Done`
  命令正常结束
- `130` -> `Cancelled`
  通常来自用户按 `Ctrl+C`
- 其他非零 -> `Failed`
  命令以错误退出
- 无 `.exit` 且窗口（window）消失 -> `Cancelled`
  通常来自用户直接关掉任务标签页，或整个会话被 kill

## 常见中断场景

`Ctrl+C`：

- 通常写出 `130`
- 任务进入 `Cancelled`
- 对应 GPU 释放

`kill-window`：

- window 消失
- 若无 `.exit`，视为 `Cancelled`
- GPU 释放

`kill-session`：

- 所有运行中的任务都会被视为结束
- 缺少 `.exit` 的任务记为 `Cancelled`
- GPU 释放

## 当前 CLI

```bash
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices auto] [--tick-seconds N]
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices 0,2,5] [--tick-seconds N]
tiny-exp-scheduler run [commands.txt] [--dry-run]
```

补充说明：

- `--dry-run` 只解析输入并解析最终 CUDA 范围，不改 tmux 标签页
- 调度器汇总会打印最终 CUDA 范围和日志目录

本项目由 AI 和人类共同编写。
