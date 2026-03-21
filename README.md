[中文](README.md) | [English](README.en.md)

# tiny-exp-scheduler

`tiny-exp-scheduler` 是一个面向单机 GPU 实验的轻量调度工具。你给它一组现成的 shell 命令，它负责在 tmux 中逐个或并发拉起这些命令、分配可用 GPU，并把输出写入日志文件（log files）。

它不生成命令，也不分析命令内容。它只负责几件很具体的事情：

- 把每一行 shell 命令当成一个任务（job）
- 占用当前 tmux 标签页（tab）作为调度器窗口
- 在同一个 tmux 会话（session）里为每个任务新开一个标签页
- 在启动时确定“本轮允许使用哪些 GPU”，然后只在这些 GPU 里分配任务
- 记录每个任务的日志和退出状态

更多细节见：[设计文档](docs/design.md)  
可运行示例见：[Workflow 示例](docs/workflows.md)

## 心智模型

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

可用 GPU 的范围只在启动时确定一次：

```text
--cuda-devices auto
    |
    +--> 启动时检查当前哪些 GPU 空闲
    +--> 把这些 GPU 记为“本轮可用”
    +--> 后续调度只使用这批 GPU
```

## 安装

依赖：

- Rust 1.74+
- `tmux`
- `nvidia-smi`

构建：

```bash
git clone git@github.com:LYK-love/tiny-exp-scheduler.git
cd tiny-exp-scheduler
cargo build --release
```

二进制：

```bash
target/release/tiny-exp-scheduler
```

## 用法

```bash
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices auto]
tiny-exp-scheduler run [commands.txt] [--logs-dir DIR] [--cuda-devices 0,2,5]
tiny-exp-scheduler run [commands.txt] [--dry-run]
cat commands.txt | tiny-exp-scheduler run --cuda-devices auto
```

运行前，你需要满足：

- 必须已经进入一个 tmux 会话（session），并位于其中一个标签页（tab）

程序启动后，会发生这些事情：

- 当前标签页会被重命名为 `__scheduler__`
- 每个任务对应的标签页会在它后面依次创建
- 结束后 `__scheduler__` 标签页会保留
- 如果当前会话里已经有 `__scheduler__`，程序直接失败

GPU 选择方式：

- `--cuda-devices auto`
  启动时通过 `nvidia-smi` 检查当前哪些 GPU 没在忙，然后把它们作为这次运行允许使用的 GPU 列表
- `--cuda-devices 0,2,5`
  只允许使用这些 GPU；只要其中有一张卡当前正在忙，程序就直接失败，不会偷偷忽略它
- `--idle-memory-threshold-mb N`
  调整“显存占用低于多少才算空闲”的阈值；默认 `64`
- 当前判断 GPU 是否空闲的标准是：
  `memory.used <= threshold`，并且 `utilization.gpu == 0`
- 启动时会打印最终采用的范围，例如：
  `Final CUDA device range: cuda:0,cuda:2,cuda:5`
- `--dry-run`
  只读取输入、检查 GPU，并打印计划，不改 tmux 标签页，不启动任务

输入规则：

- 忽略空行
- 忽略以 `#` 开头的行
- 每个剩余行就是一个任务

## 最小示例

先进入 tmux：

```bash
tmux new-session -s exp
```

然后运行：

```bash
tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices 0
```

更像 deep learning 的示例：

- [examples/torch_hold_gpu.py](examples/torch_hold_gpu.py)
- [examples/torch-two-gpu-jobs.txt](examples/torch-two-gpu-jobs.txt)
- [examples/torch-four-gpu-jobs.txt](examples/torch-four-gpu-jobs.txt)

这些例子使用最小的 `torch` 代码，在单个 GPU 上大约占用 2000 MB 显存，并持续几十秒，适合拿来观察调度行为。

## 日志与状态

默认输出：

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

任务结束后，程序会根据[退出状态码（exit status / exit code）](https://en.wikipedia.org/wiki/Exit_status)判断状态：

- `0` => `Done`
  命令正常结束
- `130` => `Cancelled`
  常见于用户在任务标签页中按 `Ctrl+C`
- 其他非零 => `Failed`
  命令运行出错
- 窗口（window）消失且无 `.exit` => `Cancelled`
  常见于用户直接关闭任务标签页，或整个会话被杀掉

所有任务结束后，`__scheduler__` 标签页会打印一段汇总信息，包括：

- 最终采用的 GPU 范围
- 日志目录
- 总任务数
- `Done / Failed / Cancelled` 数量
- `Failed / Cancelled` 的任务编号

## 测试

```bash
cargo test
bash scripts/tmux-smoke.sh
```

## License

MIT

本项目由 AI 和人类共同编写。
