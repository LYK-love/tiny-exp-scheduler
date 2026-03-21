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

快速流程图：

```text
launch tab
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

- 当前 tab 变成 `__scheduler__`
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

- 当前 tab 变成 `__scheduler__`
- 出现 `job_1` 到 `job_4`
- 四个 job 几乎同时启动

## 3. Deep Learning 风格 GPU 占用

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
- 每个 job 绑定一个 GPU

## 4. 不在 tmux 中启动

命令：

```bash
./target/release/tiny-exp-scheduler run examples/basic-queue.txt --cuda-devices auto
```

预期：

- 直接报错
- 错误说明必须在已有 tmux session 内运行

## 5. Dry Run

命令：

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices auto --dry-run
```

预期：

- 打印最终 CUDA 范围
- 打印 job 数和 logs 目录
- 不改 tmux tab
- 不启动任何 job

## 6. 运行中按 Ctrl+C

命令：

```bash
./target/release/tiny-exp-scheduler run examples/four-jobs.txt --cuda-devices 0,1,2,3
```

操作：

- 进入某个运行中的 job tab
- 按 `Ctrl+C`

预期：

- 对应 job 进入 `Cancelled`
- 对应 `.exit` 文件为 `130`
- GPU 被释放
- 其他 job 继续运行

检查：

```bash
cat logs/job_2.exit
```

## 7. 直接关闭一个 job tab

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
- 其他 job 继续运行

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

- 所有运行中 job 被视为结束
- 缺少 `.exit` 的 job 视为 `Cancelled`
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
