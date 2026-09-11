# ezStreamer

TopazChat配信専用の軽量ストリーマー (Windows 10 2004+ / 11 + Linux/Flatpak・GStreamer版)。
「起動 → 画面/音声選択 → 配信開始 → URLコピー」で完結します。

ezTopaz (FFmpeg sidecar / Win+Linux) の後継として、エンコード・多重化・RTMP送信を
プロセス内 GStreamer パイプライン (`gstreamer-rs`) に置換したもの。
GUI は Tauri + React をやめ、ネイティブの **egui/eframe** (Rust) で描画します。
キャプチャ (Windows: WGC画面 / WASAPI音声、Linux: Portal ScreenCast / PipeWire音声)・
Rust Mixer/FramePacer は従来どおりです。

## 状態

- 配信中機能: 設定管理・GStreamerパイプライン計画/レジストリprobe・音声ミキサ・FramePacer・
  GStreamer supervisor (bus監視+F-ST-04自動再接続)・配信前プレビュー (640x360@1fps)・
  egui UI一式 (ja/en)・キャプチャバックエンド (WGC+WASAPI / Portal+PipeWire)
- GUI は **egui 0.32**: 直線的でつながりのあるフラットデザイン (角丸/グラデーション/絵文字なし)、
  日本語フォント同梱 (Noto Sans JP subset)
- **Windows / Linux 実機での E2E 確認が次の必須ステップ** (CI はコンパイル + Rustテスト + バンドル検証のみ)
- リリースビルド: CI `Release` が公式 GStreamer MSVC ランタイムのサブセットを
  `ezstreamer-app/resources/gstreamer/` へ配置して NSIS に同梱し、Linux は Flatpak (GNOME runtime) を生成

## 開発

```bash
cargo test -p ezstreamer-core          # 純粋ロジック (config/pipeline/probe/mixer/pacer)
cargo check -p ezstreamer              # バックエンド (Linux は GStreamer/PipeWire dev が必要)
cargo test -p ezstreamer               # バックエンド (PipeWire fail-fast など)
cargo run -p ezstreamer                # デスクトップアプリ起動
cargo check -p ezstreamer --no-default-features   # UIのみ (GStreamer不要)
```

- Windows: GStreamer MSVC 64-bit ランタイム必須 (詳細は `docs/design.md` §13.2)
- Linux: Wayland + xdg-desktop-portal + PipeWire。Ubuntu 24.04 の依存は
  `packaging/flatpak/README.md` 参照
- Windows インストーラ (NSIS) は CI `Release` が `packaging/windows/ezstreamer.nsi`
  から生成します

## 制約と注意

- TopazChat の上限: 映像 2000kbps / 音声 320kbps (超過で強制切断)。アプリ内でガードします
- StreamKey は公開情報 (視聴URLの一部) のため平文保存します
- TopazChat は個人運営・試験運用のため、映像配信は予告なく停止する可能性があります
- x264 `zerolatency` tune と NVENC `Low Latency` preset は使用しません
  (VRChat側灰色画面の既知不具合のため。NVENC は `high-performance` を使用)
- Linux の画面選択は OS の Portal ピッカー経由のみ (アプリ側のウィンドウ一覧なし)。
  カーソル表示はピッカー選択時に決まります
- Flatpak 版の音声キャプチャは native PipeWire socket へのアクセスを必要とします
  (manifest で `--filesystem=xdg-run/pipewire-0` を要求)

## ライセンス

- ezStreamer: MIT (`LICENSE`)
- 同梱 GStreamer ランタイム: LGPL 2.1+ — `ezstreamer-app/resources/licenses/GSTREAMER-NOTICE.txt` 参照

## 支援

TopazChat 運営のよしたか氏: https://tyounanmoti.fanbox.cc/
