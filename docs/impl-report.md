# ezStreamer 引き継ぎ書 (v0.2 — レビュー修正反映後)

- 対象: TopazChat 配信専用ストリーマー (Windows + Linux/Flatpak・GStreamer)。ezTopaz (FFmpeg/Win+Linux) からのfork
- 本書: v0.2 (2026-09-10)。実装レビュー (PR #15) の修正を反映
- 設計: `docs/design.md` (v0.2) / 要件: `docs/requirements.md` (v0.2)

## 1. 現状サマリ

- **CI**: `CI` = windows-latest + ubuntu-24.04 の2ジョブ (core/backend test + frontend)。
  `Release` = Windows NSIS (GStreamer同梱) + Linux Flatpak を PR とタグでビルドし、タグ時のみ Release 公開
- 実装済み: 設定管理/自動永続化 / GStreamerパイプライン計画・レジストリprobe /
  音声ミキサ / FramePacer / GStreamer supervisor (bus監視+F-ST-04) / 配信前プレビュー /
  UI一式 / WGC+WASAPI キャプチャ / Portal+PipeWire キャプチャ / ファイルログ /
  カーソルON/OFF / per-app 音量・ミュート / プロファイル入出力
- **Windows / Linux 実機 E2E が次の必須ステップ** (CI はコンパイル + Rustテスト + バンドル検証のみ)

## 2. レビュー修正 (PR #15, 2026-09-10)

| 分類 | 内容 |
|---|---|
| エンコーダ | NVENC preset を要件準拠 `hp` (High Performance) に (Low Latency禁止)。`profile=high`/`rc-lookahead=0`/`zerolatency=false`。x264 に `pass=cbr`/`profile=high`。openh264enc は bitrate を bps、GOP を `gop-size` で要素別に解決 |
| A/V同期 | 映像PTSの固定増分を廃止し appsrc `do-timestamp` (pacerのtick落ちで恒久ズレしない) |
| メモリ | appsrc を `leaky-type=downstream` + `max-buffers` で bounded 化 |
| 永続化 | 画面/音声/プロファイル/Encoder/Ingest/Key/ロケール/カーソルを400ms debounceで自動保存・起動時復元 (F-CF-02/05) |
| ログ | `logs/ezStreamer-YYYY-MM-DD.log` (ローカル日付・10MBローテーション)。bus ERROR/EOS・リトライ・キャプチャ失敗を記録 (F-CF-04) |
| 音声 | マイクデバイス選択 (Windows GetDevice / Linux Audio/Source + TARGET_OBJECT)、不在時エラー。per-app 音量/ミュートUI (F-AU-04)。mic gainがアプリgainを上書きするバグ修正 |
| UI | 再接続中表示+キャンセル (F-ST-04)、エンコード後ビットレート/ドロップ表示 (F-ST-03)、カーソルトグル (F-SC-04)、カスタムプロファイル選択/編集/追加 (F-EN-02)、JSON export/import (F-CF-03) |
| その他 | WGC停止フラグの死コード修正、Flatpakにnative PipeWire socket権限、アプリ終了時停止、音声0ソースのエラー化、probeのlibx264常時usable撤廃、key/ingestのバックエンド検証 |

## 3. 検証状態

| 検証 | 方法 | 結果 |
|---|---|---|
| core テスト | `cargo test -p ezstreamer-core` | 69/69 |
| フロント | `pnpm build` / `pnpm test` | OK (tsc strict / vitest 14) |
| Windows check + backend test | CI (`windows-latest` + GStreamer MSVC) | PR #15 で green |
| Linux check + backend test | CI (`ubuntu-24.04` + GStreamer/PipeWire) | PR #15 で green |
| Release bundle (NSIS + Flatpak) | CI `Release` PR run | PR #15 で green |
| **実機 E2E (Win/Linux)** | 実機 | **未実施 — 次の必須ステップ** |

## 4. 残タスク (優先順)

1. **Windows実機 E2E** (AC-04/05/06/08/11): 起動 → 画面選択 → 配信開始 →
   `ffprobe rtspt://topaz.chat/live/<key>`。問題時は `%APPDATA%\ezStreamer\logs\ezStreamer-*.log`
2. **Linux/Flatpak 実機 E2E** (AC-12): Portalピッカー → 音声 (system/apps/mic) → 配信 →
   `ffprobe`。音声は native PipeWire 権限の確認を含む
3. **HWエンコーダ実機調整**: NVENC preset は `hp` に修正済み (PR #20。未知名は panic せず
   warn スキップ)。QSV/AMF の低遅延挙動を Windows 実機で確定
4. 小口: F-EN-06 詳細引数欄 (design §15でMVP見送り) / Windowsの `list_audio_devices` が
   既定レンダー端点のセッションのみ列挙する点 / `src-tauri` の lib/bin 二重定義整理
5. `cargo fmt --check` / clippy のCI強制 (現状未強制。リポジトリ全体の整形を伴う別 chore PR 推奨)
6. 次回 `preview.*` リリース (AGENTS.md §7。ユーザー指示がある時のみ)

## 5. ファイルマップ (v0.2)

```
.github/workflows/ci.yml      windows + linux (core/backend test + frontend)
.github/workflows/release.yml NSIS + Flatpak、タグで Release 公開
ezstreamer-core/
 └ gst/
    ├─ pipeline.rs  StreamPlan/build_plan/build_launch_string/EncoderSpec+props
    │               (gst_props_for: 要素別の単位/プロパティ)
    ├─ probe.rs     probe_with_elements/probe_with/pick_best (レジストリlookup)
    └─ supervisor.rs MAX_RETRIES=3/retry_backoff_ms/runtime_search_roots
src-tauri/
 ├─ src/logging.rs 日付別ファイルログ (F-CF-04)
 ├─ capture/windows/  WGC+WASAPI (micデバイス選択対応)
 ├─ capture/linux/    Portal ScreenCast + PipeWire (Audio/Source列挙)
 ├─ ipc/commands.rs   同一コマンド面 + start_portal_picker + shutdown
 └─ ipc/gst_stream.rs GstStream/spawn_pipeline/bus監視/ensure_bundled_runtime
src/                  React UI (store永続化、per-app音量、再接続UI、プロファイルCRUD)
packaging/flatpak/    Flatpak manifest (GNOME runtime + pipewire socket)
```

## 6. skills引継ぎ

- `ci-watch`: そのまま。成功→自動マージの既定動作も変わらない
- `parallel-worktree`: そのまま
- `preview-release`: NSIS + Flatpak、`resources/licenses`、LGPL注意
- `e2e-stream-test`: Windows向けに加え Linux/Flatpak 手順を拡充する余地 (AC-12)
- `tauri-ui-debug`: そのまま
