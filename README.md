[中文](README.md) | [English](README.en.md)

# tiny-exp-scheduler

`tiny-exp-scheduler` 是一个最小、明确的 tmux 任务调度工具，面向单机 GPU / deep learning 实验。

它不生成命令，也不理解命令语义。它只做三件事：

- 把每一行 shell 命令当成一个 job
- 在当前 tmux session 里为每个 job 开一个 tab
- 在一个冻结的 CUDA device 范围内做调度，并记录日志

更多细节见：[设计文档](docs/design.md)  
可运行示例见：[Workflow 示例](docs/workflows.md)

## 心智模型

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

启动时只做一次 GPU 选取：

```text
--cuda-devices auto
    |
    +--> detect idle GPUs once
    +--> freeze that set
    +--> use only that set for the whole run
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

运行前提：

- 必须已经在一个 tmux session 里
- 当前 tab 会被重命名为 `__scheduler__`
- job tab 会在它后面追加创建
- 结束后 `__scheduler__` tab 会保留
- 如果当前 session 里已经有 `__scheduler__`，程序直接失败

CUDA 选择：

- `--cuda-devices auto`
  启动时通过 `nvidia-smi` 找出当前空闲 GPU，并冻结为本次运行范围
- `--cuda-devices 0,2,5`
  只允许使用这些 GPU；只要其中一个当前不空闲，就直接失败
- `--idle-memory-threshold-mb N`
  调整空闲判定中的 `memory.used` 上限；默认 `64`
- 当前空闲判定：
  `memory.used <= threshold` 且 `utilization.gpu == 0`
- 启动时会打印最终采用的范围，例如：
  `Final CUDA device range: cuda:0,cuda:2,cuda:5`
- `--dry-run`
  只解析输入并解析 GPU 范围，不改 tmux tab，不启动 job

输入规则：

- 忽略空行
- 忽略以 `#` 开头的行
- 每个剩余行就是一个 job

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

这些例子使用最小 `torch` 代码，在单个 GPU 上大约占用 2000 MB 显存并保持几十秒。

## 日志与状态

默认输出：

```text
logs/
  job_1.log
  job_1.exit
  job_2.log
  job_2.exit
```

退出码映射：

- `0` => Done
- `130` => Cancelled
- 其他非零 => Failed
- window 消失且无 `.exit` => Cancelled

scheduler 结束时会在 `__scheduler__` tab 打印汇总：

- 最终 CUDA device 范围
- logs 目录
- 总 job 数
- Done / Failed / Cancelled 数量
- Failed / Cancelled 的 job id

## 测试

```bash
cargo test
bash scripts/tmux-smoke.sh
```

## License

MIT

本项目由 AI 和人类共同编写。
