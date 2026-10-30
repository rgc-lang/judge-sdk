#!/usr/bin/env bash
# rgc-agent 安装脚本：下载最新 release 到 ~/.local/bin。
#   curl -fsSL https://github.com/rgc-lang/judge-sdk/releases/latest/download/install.sh | bash
set -euo pipefail

REPO="rgc-lang/judge-sdk"
ASSET="rgc-agent-x86_64-linux.tar.gz"
BIN_DIR="${RGC_AGENT_BIN_DIR:-$HOME/.local/bin}"

need() { command -v "$1" >/dev/null 2>&1 || { echo "缺少 $1" >&2; exit 1; }; }
need curl
need tar

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "下载 $REPO 最新版本…"
curl -fsSL "https://github.com/$REPO/releases/latest/download/$ASSET" -o "$tmp/$ASSET"
tar -xzf "$tmp/$ASSET" -C "$tmp"

mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/rgc-agent" "$BIN_DIR/rgc-agent"

echo "已安装到 $BIN_DIR/rgc-agent"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "提示：把 $BIN_DIR 加进 PATH" ;;
esac

"$BIN_DIR/rgc-agent" hardware || true
