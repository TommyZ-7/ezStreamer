# ezStreamer

TopazChat配信専用の軽量ストリーマー (Windows専用・GStreamer版)。
「起動 → 画面/音声選択 → 配信開始 → URLコピー」で完結します。

ezTopaz (FFmpeg sidecar / Win+Linux) の後継として、対象を Windows 10 2004+ / 11 に絞り、
エンコード・多重化・RTMP送信をプロセス内 GStreamer パイプライン (`gstreamer-rs`) に置換したもの。
キャプチャ (WGC画面 / WASAPI音声)・Rust Mixer/FramePacer・UI は ezTopaz と同一機能です。

## 状態

- 初期実装: 設定管理・GStreamerパイプライン計画/レジストリprobe・音声ミキサ・FramePacer・GStreamer supervisor (bus監視+F-ST-04自動再接続)・配信前プレビュー (`start_preview`, 640x360@1fps)・UI一式・キャプチャバックエンド (WGC+WASAPI)
- **Windows 実機での動作確認が次の必須ステップ** (CI はコンパイル検証のみ)
- リリースビルド: CI `Release` が公式 GStreamer MSVC ランタイムのサブセットを `src-tauri/resources/gstreamer/` へ配置して NSIS に同梱

## 開発

```bash
pnpm install
cargo test -p ezstreamer-core   # 純粋ロジック (config/pipeline/probe/mixer/pacer)
cargo check -p ezstreamer        # Tauri glue (非Windowsはstub側のみ)
pnpm test                        # UIユーティリティ (vitest)
pnpm tauri dev                   # デスクトップアプリ起動 (Windows + GStreamer MSVC要)
pnpm tauri build                 # リリースビルド (GStreamer同梱は CI で配置)
```

要件: Windows 10 2004+ / 11。GStreamer MSVC 64-bit ランタイム必須 (詳細は `docs/design.md` §13.2)。

## 制約と注意

- TopazChat の上限: 映像 2000kbps / 音声 320kbps (超過で強制切断)。アプリ内でガードします
- StreamKey は公開情報 (視聴URLの一部) のため平文保存します
- TopazChat は個人運営・試験運用のため、映像配信は予告なく停止する可能性があります
- x264 `zerolatency` tune は使用しません (VRChat側灰色画面の既知不具合のため)

## ライセンス

- ezStreamer: MIT (`LICENSE`)
- 同梱 GStreamer ランタイム: LGPL 2.1+ — `src-tauri/resources/licenses/GSTREAMER-NOTICE.txt` 参照

## 支援

TopazChat 運営のよしたか氏: https://tyounanmoti.fanbox.cc/
