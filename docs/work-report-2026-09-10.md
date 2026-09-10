# ezStreamer 修正作業レポート / 引き継ぎ (2026-09-10)

対象レビュー: `docs/code-review-2026-09-10.md`（main @ `cc16996` 時点の指摘）
作業環境: Linux（GStreamer/PipeWire dev パッケージなし。core のみローカル検証、backend は CI 担保）

## 0. 現在の状態サマリ

| フェーズ | 内容 | 状態 |
|---|---|---|
| 1. 即修正 | NVENC preset（Critical）+ プロパティ適用 panic-safe 化 | ✅ PR #20 マージ済み |
| 2. 実機 E2E 前 | PipeWire stride / 音声全停止時の無音パディング（High） | ✅ PR #21 マージ済み |
| 3. 次 PR | retry/stop 競合・プロファイル検証・feeder オーファン・clippy | ❌ 未着手（§3 が計画） |
| 4. 余裕時 | 性能系（コピー削減・staging 再利用・RT 無ロック化等） | ❌ スコープ外 |
| docs | requirements / impl-report の追随 | ❌ 未着手（§3.9） |

- main: `92d1d29`（PR #21 merge。PR #20 の `d278185` を含む）
- PR #21: <https://github.com/TommyZ-7/ezStreamer/pull/21>
  - 2026-09-10 08:55 UTC に `CI`（changes / windows / linux）+ `Release`（build / flatpak）全緑 → マージ済み
- 作業 worktree / branch は整理済み（local + remote 削除）
- 次環境の最初の作業: 更新済み main（`92d1d29`）から §3 を開始

---

## 1. 即修正フェーズ（完了）

- PR #20 `fix/nvenc-preset-panic-safe` / commit `8ecd2e9` / merge `d278185`
- 変更内容:
  - `ezstreamer-core/src/gst/pipeline.rs`
    - NVENC `preset` を存在しない `high-performance` → 実在ニック `hp`（High Performance）に修正
    - 要件 §2.2 の "Max Performance" に相当。`p1~7 + tune=ultra-low-latency` は low-latency 系（VRChat 灰色画面）禁止要件があるため不採用
    - 実在ニック一覧を検証する回帰テスト `nvenc_preset_value_is_a_documented_nick` を追加
  - `src-tauri/src/ipc/gst_stream.rs`
    - `apply_string_props` を追加: `find_property` → `GstValueExt::deserialize_with_pspec` → `set_property`
    - デシリアライズ失敗は warn ログ + スキップ（`set_property_from_str` の panic でアプリ全体が落ちる経路を構造的に排除）
    - 不正 enum 値で panic しない backend テストを追加（appsrc `leaky-type`）
- 検証: `cargo test -p ezstreamer-core` 62/62、CI + Release 全ジョブ緑

## 2. 実機 E2E 前フェーズ（PR #21・マージ待ち）

branch `fix/pre-e2e-robustness` / commit `2ff5941` / 4 files +172 -9

- Linux/PipeWire の stride 無視を修正:
  - `ezstreamer-core/src/video/sink.rs`: `scale_bgra_strided` + `repack_bgra_rows` を追加（行 padding を正しく読む。短いチャンクは `None`）
  - `ezstreamer-core/src/video/mod.rs`: 公開
  - `src-tauri/src/capture/linux/screen.rs`: `Chunk::stride()/offset()` を使用。`Data::chunk()` 非 null ガード付き。短バッファは warn + フレーム破棄
- 音声全停止時の mux ストールを修正:
  - `ezstreamer-core/src/audio/sink.rs`: 入力キューが空のとき BLOCK（960 samples = 10ms）の無音を emit。受信タイムアウト 50ms → `BLOCK_MS`（10ms）で実時間ペース
  - flvmux（GstAggregator）が音声パッド待ちで全出力停止 → 映像固まり・ERROR/EOS なしで F-ST-04 不発、を構造的に排除
  - テスト: stride 3 件 + `emits_silence_after_all_sources_stop` 1 件、既存 `stopped_source_drops_out` をタイミング非依存化
- 検証: core 66/66、clippy 警告数は変更前と同じ 8 件（新規なし）

### PR #21 完了（実施済み）

CI（changes / windows / linux）+ Release（build / flatpak）全緑を確認してマージ済み（main `92d1d29`）。worktree `../ezStreamer-pre-e2e` と branch `fix/pre-e2e-robustness`（local + remote）は削除済み。

---

## 3. 次 PR フェーズ計画（未着手）

branch 例 `fix/lifecycle-validation-chore`、worktree `../ezStreamer-lifecycle`（更新済み main から）。
※行番号は PR #21 マージ後の main 時点。

### 3.1 retry/stop 競合（Medium）

- `src-tauri/src/ipc/commands.rs:470-509`（`spawn_retry_thread`）と `:402-418`（`stop_stream`）
- 問題: `launch_pipeline`（数百 ms）中に `stop_stream` が完了すると、後から新しい配信が `state.stream` に入り「見えないライブ配信」が残る
- 修正方針（残競合なし）: install を `stream` ロック保持で行い、その中で `retrying` を再確認する。ロック順は `stream → retrying`（`get_status` と同じ。`stop_stream` は `retrying` を即解放してから `stream` を取るためデッドロックしない）

```rust
Ok(proc) => {
    let mut stream = state.stream.lock().unwrap();
    if state.retrying.lock().unwrap().is_none() {
        drop(stream);
        proc.stop();
        stop_capture_backends(&state);
        return;
    }
    *stream = Some(proc);
    *state.retrying.lock().unwrap() = None;
    crate::logging::info(&format!("stream retry {n}/{MAX_RETRIES}: reconnected"));
    return;
}
```

- 注意: レビュー提案の「launch 後に retrying を 1 行再チェック」は check → install 間に窓が残るため不採用（本修正で原子化）
- テスト: Tauri `State` 依存の競合再現は難しいため、コードレビュー + 手動確認とする

### 3.2 プロファイル入力検証（Medium）

- `ezstreamer-core/src/gst/pipeline.rs:242` `build_plan` の先頭で検証を追加し `Error::Config`:
  - `fps >= 1`
  - `w > 0 && h > 0`、`w % 2 == 0 && h % 2 == 0`
  - HW 共通下限 `w >= 160 && h >= 128`（nvenc 160x64 / amf 128x128 / qsv 16x16 の共通下限）
- 注意: レビューの「fps=0 → appsrc caps `framerate=0/1` で交渉失敗」は過大。0/1 は raw caps として合法（vah264enc の caps も `[0/1, …]`）。実害は `gop()=0`（無限 GOP）と pacing。検証追加自体は妥当
- `src/components/SettingsModal.tsx:238` `NumberCell` に `min`（デフォルト 1）を追加
- テスト: fps=0 / 奇数 / 小さすぎ → Err、既存プロファイル → Ok

### 3.3 feeder オーファン（Low #4）

- `src-tauri/src/ipc/gst_stream.rs`: video feeder spawn（`:288-309`）成功後に audio feeder（`:313-334`）や bus thread（`:342-394`）の spawn が失敗すると `stop` が立たず、video feeder が `wait_for_playing` で永久待機（pipeline/スレッドリーク）
- `.map_err` 内で `stop.store(true, Ordering::Relaxed)` を実行（3 箇所）。spawn 失敗は稀だが確実に解放

### 3.4 encoder_override 空文字の正規化（Low #3）

- `ezstreamer-core/src/gst/pipeline.rs:227` `resolve_encoder`: `encoder_override.trim()` が空なら `"auto"` と同じ扱い（+ core テスト）
- `src-tauri/src/ipc/commands.rs:305`: usable リスト条件を `trim().is_empty()` に揃える（現状は `is_empty()` のみで、空白のみ文字列が `EncoderNotAvailable("")` になる非対称）
- ログ（`:324-329`）は正規化後の値を出力

### 3.5 store の backendError クリア（Low #10）

- `src/store.ts:396`（startStream 成功時）に `backendError: null` を追加。`startPreview`（`:424`）も同様
- 変更後 `pnpm build && pnpm test`（store テストあり）

### 3.6 gstreamer-video 依存削除（Low #8）

- `src-tauri/Cargo.toml` の windows / linux 両セクションから `gstreamer-video = "0.23"` を削除（コード使用なし。`cargo tree -i gstreamer-video` で direct 依存のみ確認済み）
- `Cargo.lock` 更新を伴う。`build.rs` の DELAYLOAD `gstvideo-1.0-0.dll` はプラグインが必要とするため残す

### 3.7 update_audio_mix の意図明示（Low #9）

- `src-tauri/src/ipc/commands.rs:583-612`: insert のみで選択解除を反映しない理由（live 中は capture 側が送り続け、`auto_register` が再登録するため remove しても効かない）をコメント化

### 3.8 clippy 8 件（Low #12）

| 場所（PR #21 後 main） | 内容 | 修正 |
|---|---|---|
| `ezstreamer-core/src/video/sink.rs:232` | `blit_nearest` 引数 9/7 | `#[allow(clippy::too_many_arguments)]`（`blit_rows` / `build_plan` と同様） |
| `ezstreamer-core/src/video/sink.rs:268` | `(oh + 63) / 64` | `oh.div_ceil(64)` |
| `ezstreamer-core/src/ipc_types.rs:62` | `impl Default for AudioMixUpdate` | `#[derive(Default)]` に統合 |
| `ezstreamer-core/src/audio/mod.rs:201,211` | `let mut m = Default; m.mic = …` | 構造体リテラル `Mixer { mic: …, ..Default::default() }` |
| `ezstreamer-core/src/config.rs:300` | `let mut cfg = Default; cfg.x = …` | 構造体リテラル |
| `ezstreamer-core/src/video/mod.rs:138,139` | `&vec![0u8; N]` | `&[0u8; N]` |

- 推奨手順: `cargo clippy -p ezstreamer-core --all-targets --fix` → 残りを手修正 → `cargo clippy -p ezstreamer-core --all-targets` で 0 件確認
- CI は clippy 未強制。強制追加は別 chore（impl-report 残タスク#5。リポジトリ全体 fmt を伴うため分離推奨）

### 3.9 docs（docs のみの別 PR 推奨。CI は軽量スキップ）

- `docs/requirements.md`
  - `:113` `preset=high-performance` → `preset=hp`（High Performance）。`Low Latency` 禁止は維持
  - `:329` 同様
  - `:106` `nvenc_h264enc` は実在確認できず（find 失敗でスキップ、実害なし）。要件記載のため今回は保留
- `docs/impl-report.md`
  - `:21` `high-performance` → `hp`
  - `:34` core テスト数 → 最終値（現在 66。§3 完了後に確定）
  - `:47-48` 残タスク#3「`nvh264enc` の `high-performance` 動作とプロパティ名…（`has_property` ガードのため起動はする）」は本レビューで修正済み。「NVENC preset は `hp` に修正済み（PR #20）。QSV/AMF の低遅延挙動を Windows 実機で確定」等に更新

### 3.10 スコープ外（レビュー priority 4「余裕時」）

- BGRA コピー削減（`FramePacer` 保持 / `pacer.push` / `Buffer::from_slice` の多重コピー。所有権受け渡し API 変更が必要）
- WGC staging texture 再利用（`src-tauri/src/capture/windows/screen.rs:212-216`）
- PipeWire RT コールバックのロックフリー化（`linux/screen.rs` の preview mutex / `scale_bgra` のスレッド生成）
- ビットレート表示の移動平均化（`gst_stream.rs` `status`）
- Low #2 `nvenc_h264enc` 削除は見送り（要件記載 + 実害なし）

---

## 4. 検証・環境メモ

- ローカル Linux に GStreamer/PipeWire dev がないため `cargo check -p ezstreamer` 不可。core はローカル、backend は CI（windows-latest + ubuntu-24.04）で担保
- テスト数推移: 61（開始時）→ 62（PR #20 後）→ 66（PR #21 後）
- `pnpm test`: 14/14（未変更）
- clippy（core, `--all-targets`）: 一意 8 件（lib 3 + test 5）
- 進め方: AGENTS.md §1（worktree）、§4（commit prefix / docs 分離）、§5（CI 緑確認）、§6（ci-watch）
  - 監視: `.opencode/skills/ci-watch/scripts/ci-watch.sh <branch> <CI|Release>` を background 起動
- `src-tauri/**` / `ezstreamer-core/**` を触る PR は Release（bundle）も PR で走る

## 5. リスク・注意

- PR #21 は Linux CI が `data.as_raw().chunk` を初コンパイル検証する。赤の場合は `src-tauri/src/capture/linux/screen.rs` の process コールバック（offset/stride 周り）を確認
- 無音パディングで mixer の受信タイムアウトを 10ms に短縮。バックログ排出が速くなるため、実機 E2E で A/V 開始同期を確認（pipeline PLAYING 前のバックログは従来から存在する挙動）
- retry 修正のロック順は `stream → retrying` を厳守。逆順は `get_status` とデッドロック
- `hp` は 1.22 で deprecated、1.28 に存在。将来削除された場合は panic-safe 化により warn スキップされ preset がデフォルトに戻る → GStreamer 更新時に p1~7 への移行を検討
- docs とコードを同 PR に混ぜない（AGENTS.md §4）

## 6. 現在のリソース

- worktree: `/home/takedatomoya/gh_projects/ezStreamer`（main のみ。作業 worktree は整理済み）
- branch: `fix/nvenc-preset-panic-safe` / `fix/pre-e2e-robustness` とも local + remote 削除済み
- PR: #20 / #21 とも merged
- 未追跡ファイル: `docs/code-review-2026-09-10.md`、本レポート（次環境で必要なら docs PR としてコミット）
