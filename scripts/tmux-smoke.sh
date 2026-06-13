#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SESSION_NAME="tiny-exp-scheduler-smoke-$$"
COMMANDS_FILE="/tmp/tiny-exp-scheduler-smoke-$$.txt"
LOG_DIR="$ROOT_DIR/logs-smoke"
FAKE_BIN_DIR="/tmp/tiny-exp-scheduler-fake-bin-$$"

cleanup() {
  tmux kill-session -t "$SESSION_NAME" >/dev/null 2>&1 || true
  rm -f "$COMMANDS_FILE"
  rm -rf "$FAKE_BIN_DIR"
}

trap cleanup EXIT

cat >"$COMMANDS_FILE" <<'EOF'
python -c "import time; print('job 1 start'); time.sleep(1); print('job 1 done')"
python -c "import time; print('job 2 start'); time.sleep(1); print('job 2 done')"
EOF

rm -rf "$LOG_DIR"
mkdir -p "$LOG_DIR"
mkdir -p "$FAKE_BIN_DIR"

cat >"$FAKE_BIN_DIR/nvidia-smi" <<'EOF'
#!/usr/bin/env bash
printf '0, 0, 0\n1, 0, 0\n'
EOF
chmod +x "$FAKE_BIN_DIR/nvidia-smi"

tmux new-session -d -s "$SESSION_NAME" -n launch
tmux send-keys -t "$SESSION_NAME:launch" "cd '$ROOT_DIR'" C-m
tmux send-keys -t "$SESSION_NAME:launch" "export PATH='$FAKE_BIN_DIR':\$PATH" C-m
tmux send-keys -t "$SESSION_NAME:launch" "cargo run -- run '$COMMANDS_FILE' --cuda-devices auto --logs-dir '$LOG_DIR'" C-m

for _ in $(seq 1 30); do
  WINDOWS="$(tmux list-windows -t "$SESSION_NAME" -F '#{window_name}')"
  if echo "$WINDOWS" | grep -q '^s$' \
    && echo "$WINDOWS" | grep -q '^j1$' \
    && echo "$WINDOWS" | grep -q '^j2$'; then
    break
  fi
  sleep 1
done

echo "$WINDOWS" | grep -q '^s$'
echo "$WINDOWS" | grep -q '^j1$'
echo "$WINDOWS" | grep -q '^j2$'

for _ in $(seq 1 30); do
  if test -f "$LOG_DIR/job_1.exit" && test -f "$LOG_DIR/job_2.exit"; then
    break
  fi
  sleep 1
done

test -f "$LOG_DIR/job_1.exit"
test -f "$LOG_DIR/job_2.exit"
grep -q '^0$' "$LOG_DIR/job_1.exit"
grep -q '^0$' "$LOG_DIR/job_2.exit"

echo "tmux smoke test passed for session $SESSION_NAME"
