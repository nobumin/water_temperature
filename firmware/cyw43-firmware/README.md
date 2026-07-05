# CYW43439 ファームウェア blob 置き場

Pico W の無線チップ CYW43439 を動かすには、以下の Infineon 提供バイナリが必要です。
**ライセンスの都合上リポジトリにはコミットしません**(`.gitignore` で除外)。

- `43439A0.bin` … Wi-Fi ファームウェア
- `43439A0_clm.bin` … CLM(規制/チャネル情報)
- `43439A0_btfw.bin` … Bluetooth ファームウェア
- `nvram_rp2040.bin` … NVRAM 設定

## 取得方法

リポジトリルートで次を実行してください:

```bash
scripts/fetch-cyw43-firmware.sh
```

または [embassy の cyw43-firmware](https://github.com/embassy-rs/embassy/tree/main/cyw43-firmware)
から手動で上記 4 ファイルをこのディレクトリへ配置します。

> blob 無しでコンパイル確認のみ行う場合は、`--features skip-cyw43-firmware` を付けてビルドします
> (空のスタブが使われ、実機では動作しません)。
