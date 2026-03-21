# Workflow 示例

## 前提

```bash
cargo build --release
cd tiny-exp-scheduler
tmux new-session -s exp
```

如果要跑 `torch` 示例，还需要：

- 已安装 `torch`
- `torch.cuda.is_available()` 为真
- 机器上有可用 GPU

运行时的标签页（tab）关系大致如下：

```text
启动命令所在标签页
  -> __scheduler__
  -> job_1
  -> job_2
  -> ...
```

## 1. 最小运行

命令：

```bash
./target/release/tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices 0
```

预期：

- 当前标签页变成 `__scheduler__`
- `job_1` 先跑
- `job_2` 等 `job_1` 结束后再跑

检查：

```bash
cat logs/job_1.exit
cat logs/job_2.exit
```

两者都应为：

```text
0
```

## 2. 四任务并发

命令：

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

预期：

- 当前标签页变成 `__scheduler__`
- 出现 `job_1` 到 `job_4`
- 四个任务几乎同时启动

## 3. 贴近深度学习实验的 GPU 占用示例

单 GPU 排队：

```bash
./target/release/tiny-exp-scheduler run examples/torch-two-gpu-jobs.txt --cuda-devices 0
```

预期：

- `job_1` 先占用 GPU
- `job_2` 等待
- `nvidia-smi` 可见显存增加约 2000 MB

四 GPU 并发：

```bash
./target/release/tiny-exp-scheduler run examples/torch-four-gpu-jobs.txt --cuda-devices 0,1,2,3
```

预期：

- `job_1` 到 `job_4` 同时启动
- 每个任务绑定一个 GPU

## 4. 不在 tmux 中启动

命令：

```bash
./target/release/tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices auto
```

预期：

- 直接报错
- 错误说明必须在已有 tmux 会话（session）内运行

## 5. Dry Run

命令：

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices auto --dry-run
```

预期：

- 打印最终 CUDA 范围
- 打印任务数和日志目录
- 不改 tmux 标签页
- 不启动任何任务

## 6. 运行中按 Ctrl+C

命令：

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

操作：

- 进入某个运行中的任务标签页
- 按 `Ctrl+C`

预期：

- 对应任务进入 `Cancelled`
- 对应 `.exit` 文件为 `130`
- GPU 被释放
- 其他任务继续运行

检查：

```bash
cat logs/job_2.exit
```

## 7. 直接关闭一个任务标签页

命令：

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

操作：

```bash
tmux kill-window -t exp:job_3
```

预期：

- `job_3` 视为 `Cancelled`
- GPU 被释放
- 其他任务继续运行

## 8. 关闭整个 session

命令：

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

操作：

```bash
tmux kill-session -t exp
```

预期：

- 所有运行中的任务被视为结束
- 缺少 `.exit` 的任务视为 `Cancelled`
- 所有 GPU 被释放

## 9. 从标准输入读取任务

命令：

```bash
cat examples/basic-queue.txt | ./target/release/tiny-exp-scheduler run --cuda-devices auto
```

语义：

- 与传文件路径等价
- 只改变输入来源

## 手工回归最小集合

```text
1. basic-queue + explicit single GPU
2. four-jobs + explicit multi-GPU
3. torch-two-gpu-jobs
4. Ctrl+C on one job
5. tmux kill-window
6. tmux kill-session
```

本项目由 AI 和人类共同编写。
