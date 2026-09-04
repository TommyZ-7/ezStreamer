# ezStreamer 引き継ぎ書 (v0.1 — 初期実装時点)

- 対象: TopazChat 配信専用ストリーマー (Windows専用・GStreamer)。ezTopaz (FFmpeg/Win+Linux) からのfork
- 本書: v0.1。初期実装完了時点の状態を引き継ぐ
- 設計: `docs/design.md` (v0.1) / 要件: `docs/requirements.md` (v0.1)

## 1. 現状サマリ

- **期待CI**: `windows` 1ジョブ (core テスト + Windows check + フロント)。Linux/Arch/deb/AppImage/AURは廃止
- 実装済み: 設定管理 / GStreamerパイプライン計画・レジストリprobe / 音声ミキサ / FramePacer / GStreamer supervisor (bus監視+F-ST-04) / 配信前プレビュー / UI一式 / WGC+WASAPIキャプチャ (ezTopazから継承)
- **Windows実機未検証**: 次の必須ステップは Windows 実機での E2E (§5)

## 2. ezTopazからの変更点 (fork差分)

| 項目 | 状態 | 内容 |
|---|---|---|
| Windows専用化 | ✅ | `capture-linux`, Portal/PipeWire, feature flags全廃止。`cfg(windows)` ゲートのみ。非Windowsはstub+明確エラー |
| FFmpeg→GStreamer | ✅ | `eztopaz-core/src/ffmpeg/` → `ezstreamer-core/src/gst/` (`pipeline`/`probe`/`supervisor`)。sidecar/named pipe/`encoders.json`キャッシュ/`ddagrab` direct入力を廃止 |
| 供給路 | ✅ | `VideoSink`/`AudioSink` に `spawn_appsrc` 追加 (paced BGRA / 混合F32LEをmpscでfeederへ)。File-drain版はpreview/debug用に残す |
| パイプライン実行 | ✅ | `src-tauri/src/ipc/gst_stream.rs` (appsrc→encode→flvmux→rtmp2sink、bus監視、プリロール400ms fail-fast) |
| ランタイム同梱 | ✅ | `ensure_bundled_runtime` (同梱優先→システムフォールバック)。`resources/gstreamer/` はCI配置、ローカルは空でOK |
| UI | ✅ | Portalピッカーボタン削除、ライセンス表記をGStreamer LGPLに。`hw_direct`/`direct_input` は読込互換のみ |
| ライセンス汚染解消 | ✅ | libx264/GPL問題が消滅 (GStreamer LGPL同梱)。`GSTREAMER-NOTICE.txt` を同梱 |
| CI | ✅ | windows単一 + NSIS Release (GStreamer subset同梱)。choco導入のため初回は時間がかかる |
| skills | ✅ | 5 skillを継承・最適化 (Linux/FFmpeg記述の除去。詳細は§6) |

## 3. 検証状態

| 検証 | 方法 | 結果 |
|---|---|---|
| core テスト | `cargo test -p ezstreamer-core` | 48/48 |
| ホストビルド | `cargo check -p ezstreamer` (Linux, stub側) | OK, 警告0 |
| フロント | `pnpm build` / `pnpm test` | OK (tsc strict / vitest 12) |
| Windows check | CI (`windows-latest` + GStreamer MSVC) | 未実行 — 初回CIで確認 |
| **実機 E2E** | Windows実機 | **未実施 — 次の必須ステップ** |

## 4. 残タスク (優先順)

1. **Windows実機 E2E** (要件 AC-04/05/06/08/11): 起動 → 画面選択 → 配信開始 → `ffprobe rtspt://topaz.chat/live/<key>`。問題時は `logs/ezStreamer-*.log`
2. **HWエンコーダ実機調整** (design §15): `nvh264enc`/`qsvh264enc`/`amfh264enc` のプロパティ名・低遅延挙動を実機で確定 (`has_property` ガードのため起動はする)
3. **同梱subset確定** (design §13.2/§15): サイズ実測→allowlist固定 (目標 NSIS <150MB)
4. 小口: F-AU-04 アプリ別ゲインUI (バックエンド `update_audio_mix` は実装済み) / ログローテート / F-CF-03 export/import / F-EN-06 詳細引数欄
5. 初回 `preview.*` リリース (AGENTS.md §7)

## 5. ファイルマップ (v0.1)

```
.github/workflows/ci.yml      windows単一 (core test + check + frontend)
.github/workflows/release.yml NSIS + GStreamer subset同梱 + タグでRelease公開
ezstreamer-core/
 └ gst/
    ├─ pipeline.rs  StreamPlan/build_plan/build_launch_string/EncoderSpec+props
    ├─ probe.rs     probe_with_elements/probe_with/pick_best (レジストリlookup)
    └─ supervisor.rs MAX_RETRIES=3/retry_backoff_ms/runtime_search_roots
src-tauri/
 ├─ capture/windows/  WGC+WASAPI (継承。direct.rsは互換shim)
 ├─ ipc/commands.rs   同一コマンド面 (portal削除、probeにAppHandle注入)
 └─ ipc/gst_stream.rs GstStream/spawn_pipeline/bus監視/ensure_bundled_runtime
src/                  React UI (Portalボタン削除、ライセンス文更新のみ)
```

## 6. skills引継ぎ (最適化内容)

- `ci-watch`: そのまま継承 (分岐名のみ例示をezStreamer化)。監視方式の知見は共通
- `parallel-worktree`: そのまま継承 (`../ezTopaz-<suffix>` → `../ezStreamer-<suffix>` に改名)
- `preview-release`: GStreamer版に更新 (deb/AppImage/FFmpeg-GPL手順を削除、NSIS+`resources/gstreamer`+LGPL注意に置換)
- `e2e-stream-test`: Windows専用に更新 (`gst-launch` launch文字列での事前疎通 + ffprobe受入。Wayland/PipeWire/`EZTOPAZ_FFMPEG`手順を削除)
- `tauri-ui-debug`: そのまま継承 (リポジトリ固有の過去事例は共通のため保持。`default.json`確認手順も同一)
