# 设计文档

如果你还没有安装或使用过这个工具，请先看 [README.md](../README.md)。如果你想直接看有代表性的命令模式，请看 [workflows.md](workflows.md)。

## 1. 系统总览

`tiny-exp-scheduler` 是一个在单机多卡环境中并行运行 shell 命令的调度器。

它默认采用“一任务一 GPU”的资源模型。因此，这个工具主要面向单卡任务；对于少数根本不需要 CUDA 分配的任务，也提供显式的 `none` 模式。

从高层看，它只做四件事：

1. 读取一组任务（job）
2. 确定本次运行允许使用的 GPU 集合
3. 在不同的 `tmux` 标签页中并行启动任务
4. 记录日志和最终状态

一个任务（job）就是输入中的一个非空、非注释行。每个任务都必须是一条完整的 shell 命令。

## 2. 顶层抽象

整个系统可以理解为五层。

### Layer 1：命令来源

调度器从两种来源读取输入：

- 命令文件
- 标准输入

输入是逐行解析的：

- 忽略空行
- 忽略以 `#` 开头的行
- 每个其余行都视为一个任务（job）

命令文件使用 shell 语法，但它本身不是一个要直接执行的 shell 脚本。

### Layer 2：调度核心

调度核心维护：

- 待运行任务队列
- 运行中任务集合
- 本次运行固定的 GPU 池
- 主调度循环

它负责把待运行任务映射到空闲 GPU，并持续跟踪状态变化，直到所有任务结束。

### Layer 3：tmux 运行时

调度器必须运行在一个已有的 `tmux` 会话（session）中。

在这个会话里：

- 当前标签页会被重命名为 `__sched__`
- 每个运行中的任务都会得到一个新的标签页
- 每个任务标签页只包含一个 pane
- 这个 pane 只运行一条 shell 命令

因此运行时形态是：

```text
command source
    |
    v
__sched__
    |
    +--> job_1
    +--> job_2
    +--> job_3
    +--> ...
```

### Layer 4：任务执行

一个运行中的任务，本质上是“原始 shell 命令 + 调度器控制的环境变量 + 日志与退出状态记录”。

概念上，每个任务会以这样的形式启动：

```text
CUDA_VISIBLE_DEVICES=<gpu_id> + raw shell command + log capture + exit capture
```

GPU 可见性由调度器从外部控制；任务命令本身仍然完全由用户定义。

### Layer 5：持久化输出

对于每个任务，调度器都会写出：

- 一个日志文件
- 一个退出状态文件

这些文件构成执行结果的持久记录，不依赖对应的 `tmux` 标签页是否还可见。

## 3. 架构

运行时架构包含五个组件。

```text
command file / stdin
        |
        v
  input parser
        |
        v
 scheduler core
    |       |
    |       +--> GPU pool manager
    |
    +--> tmux tab launcher
    |
    +--> status collector
    |
    +--> logs / exit files
```

各组件职责如下：

- *input parser*：把输入行规范化为任务
- *GPU pool manager*：确定本次运行允许使用哪些 GPU
- *scheduler core*：把任务分配到空闲 GPU
- *tmux tab launcher*：在 `tmux` 中启动任务标签页
- *status collector*：判断任务何时结束，并推导最终状态

## 4. 工作流程

运行过程分为三个阶段。

### Phase 1：启动阶段

启动时，调度器会：

1. 检查当前是否运行在 `tmux` 中
2. 从文件或标准输入读取任务
3. 确定本次运行允许使用的 GPU 集合
4. 检查启动条件是否合法
5. 把当前标签页改名为 `__sched__`

以下情况会在启动阶段直接失败：

- 不在 `tmux` 中运行
- 当前会话中已经存在 `__sched__`
- 在当前模式下没有可用 GPU
- 用户显式指定的某张 GPU 当前处于忙碌状态

### Phase 2：调度循环

启动完成后，调度器进入轮询循环。

每次迭代执行以下步骤：

```text
1. 在固定 GPU 池中查找空闲 GPU
2. 为待运行任务分配这些 GPU
3. 在 tmux 标签页中启动新分配的任务
4. 检查运行中的任务是否结束
5. 回收已结束任务占用的 GPU
6. 休眠到下一次 tick
```

只要还有待运行任务或运行中任务，这个循环就会继续。

### Phase 3：结束阶段

当所有任务都结束后，调度器会：

1. 计算最终任务状态
2. 在 `__sched__` 中打印汇总
3. 保留 `__sched__` 标签页

已结束的任务标签页是否仍然可见，取决于 `--keep-job-tabs`。

## 5. GPU 工作流程

GPU 管理采用“固定 GPU 池”的模型。

### Step 1：确定 GPU 池

调度器支持三种模式：

```text
--cuda-devices auto
--cuda-devices none
--cuda-devices 0,2,5
```

如果使用 `auto`：

- 启动时通过 `nvidia-smi` 检查 GPU
- 收集当时空闲的 GPU
- 将这个集合固定为本次运行的 GPU 池

如果使用显式列表：

- 只使用用户列出的 GPU
- 只要其中任意一张卡在启动时忙碌，就直接失败

如果使用 `none`：

- 调度器不分配 GPU
- 调度器不设置 `CUDA_VISIBLE_DEVICES`
- 任务不再受 GPU 槽位数量限制

### Step 2：调度期间只使用这个池

调度器不会在运行过程中扩大 GPU 池。某张 GPU 即使在启动之后才变空闲，只要它不属于启动时确定的 GPU 池，就不会参与这次运行。

### Step 3：任务结束后回收 GPU

当一个运行中的任务结束后，它占用的 GPU 会回到同一个池的空闲子集中，可以继续分配给后续任务。

`auto` 模式使用的空闲判定规则是：

```text
memory.used <= threshold
and
utilization.gpu == 0
```

其中 `threshold` 由 `--idle-memory-threshold-mb` 控制。

## 6. 任务工作流程

每个任务有两类状态：

- 执行状态：供调度循环使用
- 最终结果状态：供用户查看

执行状态机是：

```text
Pending -> Scheduled -> Running -> Finished
```

最终结果状态是：

- `Done`
- `Failed`
- `Cancelled`

完整关系如下：

```text
Pending -> Scheduled -> Running -> Finished -> {Done | Failed | Cancelled}
```

这里的 `Finished` 只表示“命令已经不再运行”。在这之后，调度器才会结合 `tmux` 运行时状态和退出状态文件，推导出最终结果。

### Pending

任务已经被解析出来，但还没有分配 GPU。

### Scheduled

调度器已经给任务分配了 GPU，但任务还没有在 `tmux` 中启动。

### Running

任务已经在自己的 `tmux` 标签页中启动，并正在占用一张 GPU。

从运行时角度看，当任务标签页和 pane 都已经创建完成，并且命令已经在其中启动后，任务进入 `Running`。

### Finished

当调度器确认命令已经不再存活时，任务进入 `Finished`。

这个判断依赖 `tmux` 运行时状态，满足以下任一条件即可：

- 任务标签页已经不存在
- 任务 pane 已经结束，即使标签页仍然存在

任务进入 `Finished` 后，调度器再按以下规则推导最终结果：

- 如果退出状态文件存在，且退出状态码为 `0`，任务记为 `Done`
- 如果退出状态文件存在，且退出状态码为 `130`，任务记为 `Cancelled`
- 如果退出状态文件存在，且退出状态码为其他非零值，任务记为 `Failed`
- 如果任务标签页已经消失，但没有找到退出状态文件，任务记为 `Cancelled`

因此，`Done`、`Failed`、`Cancelled` 不是和 `Running` 并列的状态；它们是任务已经 `Finished` 之后的最终分类。

## 7. tmux 工作流程

调度器把 `tmux` 同时当成运行容器和任务状态检测的一部分。

### 调度器标签页

当前标签页会被改名为 `__sched__`。它承载控制循环，并在最后显示汇总。

### 任务标签页

每个启动的任务都会得到一个名为 `job_X` 的标签页。

每个任务标签页只包含一个 pane，而这个 pane 只运行一条命令。

### 通过 tmux 判定状态

对于每个运行中的任务，调度器都会跟踪对应的 `tmux` 标签页和 pane。

在每次调度 tick 中，它会检查：

- 这个标签页是否仍然存在
- 这个 pane 是否仍然存活

这些检查用于判断任务是否仍在运行，还是已经到达 `Finished`。

对应关系是：

- 标签页存在，pane 仍存活 -> 任务仍为 `Running`
- 标签页存在，但 pane 已结束 -> 任务已到达 `Finished`
- 标签页不存在 -> 任务已到达 `Finished`

在这之后，调度器再读取退出状态文件，进一步推导 `Done`、`Failed` 或 `Cancelled`。

### 完成后的标签页行为

默认情况下：

- 已结束的任务标签页会退出并消失

如果启用 `--keep-job-tabs`：

- 已结束的任务标签页会保留，便于检查
- 但 pane 中的命令进程已经结束
- 任务也已经到达 `Finished`

因此，标签页是否仍然可见，和任务是否还在运行，并不是同一件事。在 `--keep-job-tabs` 模式下，某个标签页仍然可见，并不意味着其中的任务还活着。

## 8. 命令展开

调度器不会理解任务本身的业务语义，但它会把原始命令包装成一个明确的运行形式。

概念上，每个任务启动时，调度器都会在原始命令外层附加三件事：

1. 通过 `CUDA_VISIBLE_DEVICES` 控制 GPU 可见性
2. 捕获日志
3. 捕获退出状态

因此运行时形态大致是：

```text
scheduler env + raw shell command + stdout/stderr capture + exit capture
```

这也是调度器唯一包裹用户命令的地方。

## 9. 状态与输出模型

默认输出结构如下：

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

最终状态的推导规则如下：

- 退出状态码为 `0` -> `Done`
- 退出状态码为 `130` -> `Cancelled`
- 其他非零退出状态码 -> `Failed`
- 退出状态文件缺失，且对应标签页也已消失 -> `Cancelled`

因此，调度器同时使用进程退出信息和 `tmux` 运行时状态。

## 10. 中断流程

中断可以来自 `tmux` 操作，也可以来自命令本身的退出。

### 在任务标签页里按 Ctrl+C

- 命令通常以 `130` 退出
- 任务记为 `Cancelled`
- 占用的 GPU 被回收

### 手动关闭任务标签页

- 标签页消失
- 如果没有退出状态文件，任务记为 `Cancelled`
- 占用的 GPU 被回收

### 整个 tmux 会话被 kill

- 所有运行中的任务都会被视为结束
- 没有退出状态文件的任务记为 `Cancelled`
- 所有 GPU 都会被回收

## 11. CLI 界面

当前命令形式：

```bash
tiny-exp-scheduler run [COMMANDS_FILE] [OPTIONS]
cat commands.txt | tiny-exp-scheduler run [OPTIONS]
```

主要选项：

- `--cuda-devices auto`
- `--cuda-devices none`
- `--cuda-devices 0,2,5`
- `--idle-memory-threshold-mb N`
- `--logs-dir DIR`
- `--tick-seconds N`
- `--keep-job-tabs`
- `--dry-run`

`--dry-run` 会解析输入和计划状态，但不会真正启动任务，也不会改动 `tmux`。
