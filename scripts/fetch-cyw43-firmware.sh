#!/usr/bin/env bash
# CYW43439 (Pico W) のファームウェア blob を embassy リポジトリから取得する。
# これらは Infineon 提供のバイナリで、リポジトリにはコミットせず各自取得する。
#
# 使い方: scripts/fetch-cyw43-firmware.sh
set -euo pipefail

DEST="$(cd "$(dirname "$0")/.." && pwd)/firmware/cyw43-firmware"
# trouble/embassy 例と同じ rev に固定して取得(Cargo.toml の patch rev と一致)。
REV="1d3c3de"
BASE="https://raw.githubusercontent.com/embassy-rs/embassy/${REV}/cyw43-firmware"

mkdir -p "$DEST"

for f in 43439A0.bin 43439A0_clm.bin 43439A0_btfw.bin nvram_rp2040.bin; do
  echo "downloading ${f} ..."
  if ! curl -fsSL "${BASE}/${f}" -o "${DEST}/${f}"; then
    echo "warning: ${f} を取得できませんでした(この rev に存在しない可能性)。" >&2
  fi
done

echo "done -> ${DEST}"
ls -l "${DEST}"
