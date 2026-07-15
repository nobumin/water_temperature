#!/usr/bin/env pwsh
# CYW43439 (Pico W) のファームウェア blob を embassy リポジトリから取得する (PowerShell 版)。
# fetch-cyw43-firmware.sh の PowerShell 相当。Windows で bash が無くても実行できる。
# これらは Infineon 提供のバイナリで、リポジトリにはコミットせず各自取得する。
#
# 使い方 (リポジトリのどこからでも可):
#   pwsh -File scripts/fetch-cyw43-firmware.ps1
# Windows PowerShell 5.1 / PowerShell 7 いずれも動作する。

$ErrorActionPreference = "Stop"

# スクリプトの2つ上の階層 = リポジトリルート。
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$dest = Join-Path $repoRoot "firmware/cyw43-firmware"
# trouble/embassy 例と同じ rev に固定して取得 (Cargo.toml の patch rev と一致)。
$rev  = "1d3c3de"
$base = "https://raw.githubusercontent.com/embassy-rs/embassy/$rev/cyw43-firmware"

New-Item -ItemType Directory -Force -Path $dest | Out-Null

$files  = @("43439A0.bin", "43439A0_clm.bin", "43439A0_btfw.bin", "nvram_rp2040.bin")
$failed = @()
foreach ($f in $files) {
    Write-Host "downloading $f ..."
    try {
        Invoke-WebRequest -Uri "$base/$f" -OutFile (Join-Path $dest $f) -UseBasicParsing
    } catch {
        Write-Warning "error: $f を取得できませんでした。"
        $failed += $f
    }
}

Get-ChildItem $dest | Format-Table -AutoSize

# 1 つでも取得に失敗したら非 0 で終了する
# (blob 不足のままビルドへ進み include_bytes! で失敗するのを防ぐ)。
if ($failed.Count -gt 0) {
    Write-Warning ("failed to download: {0}" -f ($failed -join ", "))
    Write-Warning "取得に失敗した blob があります。ネットワークや rev($rev) を確認してください。"
    exit 1
}

Write-Host "done -> $dest"
