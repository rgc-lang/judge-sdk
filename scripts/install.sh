#!/usr/bin/env bash
# rgc-agent 安装脚本：自动识别架构，装到 ~/.local/bin。
#   curl -fsSL https://github.com/rgc-lang/judge-sdk/releases/latest/download/install.sh | bash
set -euo pipefail

REPO="${RGC_AGENT_REPO:-rgc-lang/judge-sdk}"
BIN_DIR="${RGC_AGENT_BIN_DIR:-$HOME/.local/bin}"

need() { command -v "$1" >/dev/null 2>&1 || { echo "缺少 $1" >&2; exit 1; }; }

case "$(uname -s)" in
  Linux) ;;
  Darwin)
    echo "macOS 没有 CUDA，rgc-agent 只能在带 NVIDIA 显卡的 Linux 上运行。" >&2
    exit 1
    ;;
  *)
    echo "暂不支持 $(uname -s)：目前只有 Linux（x86_64 / aarch64）。" >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *)
    echo "暂不支持 $(uname -m) 架构：目前只有 x86_64 / aarch64。" >&2
    exit 1
    ;;
esac

need curl
need tar

ASSET="rgc-agent-linux-${ARCH}.tar.gz"
URL="https://github.com/$REPO/releases/latest/download/$ASSET"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "下载 $ASSET …"
curl -fsSL "$URL" -o "$tmp/$ASSET"
tar -xzf "$tmp/$ASSET" -C "$tmp"

mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/rgc-agent" "$BIN_DIR/rgc-agent"

echo "已安装到 $BIN_DIR/rgc-agent"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "提示：把 $BIN_DIR 加进 PATH" ;;
esac

"$BIN_DIR/rgc-agent" hardware || true
