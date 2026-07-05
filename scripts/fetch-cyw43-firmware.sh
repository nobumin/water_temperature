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

failed=()
for f in 43439A0.bin 43439A0_clm.bin 43439A0_btfw.bin nvram_rp2040.bin; do
  echo "downloading ${f} ..."
  if ! curl -fsSL "${BASE}/${f}" -o "${DEST}/${f}"; then
    echo "error: ${f} を取得できませんでした。" >&2
    failed+=("${f}")
  fi
done

ls -l "${DEST}"

# 1 つでも取得に失敗したら非 0 で終了する
# (blob 不足のままビルドへ進み include_bytes! で失敗するのを防ぐ)。
if (( ${#failed[@]} > 0 )); then
  echo "failed to download: ${failed[*]}" >&2
  echo "取得に失敗した blob があります。ネットワークや rev(${REV}) を確認してください。" >&2
  exit 1
fi

echo "done -> ${DEST}"
