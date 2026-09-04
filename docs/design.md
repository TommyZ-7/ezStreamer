# ezStreamer 詳細設計書 v0.1

> 作成日: 2026-09-04 | 要件定義書: `docs/requirements.md` v0.1 対応 | ステータス: Draft
> 派生元: ezTopaz 詳細設計書 v0.2。キャプチャ・ミキシング・UIは継承し、FFmpeg sidecar連携 (§4) とエンコーダprobe (§8.1) をGStreamer化、Linux系 (§3.1.2/§3.2.2ほか) を削除。

---

## 1. 設計方針

- **YAGNI徹底:** 録画/シーン合成/自動更新/テレメトリは作らない。画面は全画面/単一ウィンドウ、音声は「システム or アプリ複数 + マイク」で割り切る
- **Windows専用:** `cfg(windows)` 以外はstub。featureフラグは廃止 (ゲートはOS条件のみ)。非Windowsホストでも `cargo test` / `cargo check` / `pnpm build` が通る
- **GStreamerはプロセス内:** sidecarプロセス・named pipe・stderrパース廃止。Rustキャプチャ → `appsrc` 2本 → エンコード → `flvmux` → `rtmp2sink`
- **フレーム供給はRustが司る:** WGCはコンテンツ変化時のみフレームを出す。Rust側で「最終フレームのfps複製送出」と「プロファイル解像度への正規化」を行い、`appsrc` のcapsを起動中不変に保つ(§3.1.3)
- **x264 `zerolatency`禁止:** Topaz灰色画面の既知不具合。 tune系は使わず B-frames 0 / GOP 2s / CBR を明示

---

## 2. システムアーキテクチャ

### 2.1 全体構成

```
┌─────────────────────────────────────────────────────────┐
│  React UI (Vite + Tailwind, ja/en)                     │
│   Header / ScreenSelector / AudioSelector /            │
│   ProfileSelector / StreamControl / LogView            │
└──────────────────────┬──────────────────────────────────┘
                        │ Tauri IPC (invoke/event)
┌──────────────────────▼──────────────────────────────────┐
│  Rust Backend (Tauri 2, Windows)                         │
│  ┌──────────────┐  ┌────────────┐  ┌─────────────────┐  │
│  │CaptureManager│──│AudioMixer  │──│  GstPipeline    │  │
│  │ WGC / WASAPI │  │(Rust合成)  │  │ (in-process)    │  │
│  └──────┬───────┘  └─────┬──────┘  └────────┬────────┘  │
│         │                │                  │            │
│   ┌─────▼─────┐    ┌─────▼─────┐    ┌──────▼──────┐     │
│   │video_src  │    │audio_src  │    │ RTMP FLV    │     │
│   │(appsrc)   │    │(appsrc)   │    │ rtmp://...  │     │
│   └───────────┘    └───────────┘    └─────────────┘     │
│  ┌───────────────────────────────────────┐              │
│  │ ConfigManager (profiles.json)         │              │
│  └───────────────────────────────────────┘              │
└─────────────────────────────────────────────────────────┘
```

### 2.2 技術スタック確定

| 層 | 技術 | バージョン/備考 |
|---|---|---|
| Backend | Tauri 2 + Rust | `tauri 2.x` |
| Frontend | React + TypeScript + Vite + Tailwind | `react 18`, `zustand` 状態管理 |
| 画面 | `windows-rs 0.52` (WGC) | 全画面 `CreateForMonitor` / ウィンドウ `CreateForWindow` |
| 音声 | WASAPI (自前, `windows` crate) | per-appはプロセスループバック (2004+) |
| エンコード〜送信 | GStreamer (`gstreamer`/`gstreamer-app`/`gstreamer-video` 0.23, MSVCランタイム同梱) | §4 |
| 設定 | `serde_json` | `%APPDATA%/ezStreamer/profiles.json` |
| i18n | `i18next` (React) | ja/en |

### 2.3 構成 (Cargo workspace)

```
ezStreamer/
 ├─ Cargo.toml            # workspace (ezstreamer-core + src-tauri)
 ├─ ezstreamer-core/      # 純粋ロジック (config / gst pipeline・probe / mixer / pacer / 共有型)
 │                        #   プラットフォーム非依存。cargo test がどのOSでも通る
 ├─ src-tauri/            # Tauri glue + Windowsキャプチャ + GstPipeline実行
 │   ├─ src/main.rs
 │   ├─ src/ipc/          # commands.rs / gst_stream.rs
 │   ├─ src/capture/      # windows/ のみ (WGC+WASAPI)
 │   └─ icons/
 ├─ src/                  # React frontend
 └─ src-tauri/resources/  # licenses/ + gstreamer/ (CIがランタイム subset を配置)
```

---

## 3. キャプチャ設計 (ezTopaz継承・Windowsのみ)

### 3.1 画面キャプチャ (WGC)

| 対象 | 実装 | 備考 |
|---|---|---|
| 全画面 | `WGC: GraphicsCaptureItem::CreateForMonitor` | マルチモニタは `HMONITOR` 列挙 |
| ウィンドウ | `WGC: CreateForWindow(HWND)` | 最小化ウィンドウは警告。`ddagrab` は廃止 (GStreamerにデバイス入力は使わず `appsrc` 統一) |
| カーソル | `GraphicsCaptureSession::IncludeCursor` | ON/OFF切替 |
| プレビュー | Rust側で 640x360 に縮小 → 1fps に間引き `base64 PNG` を `event` でReactへ送信 | GStreamer経由しない |

### 3.1.3 フレーム供給ポリシー

WGCはフレームをコンテンツ変化時にのみ供給し、かつソースサイズは起動中に変わりうる。`appsrc` のcaps整合を保つため、キャプチャと供給の間に**FramePacer**を置く。

```
[WGC] --変化時のみ--> FramePacer --正規化--> video_rx (mpsc) --feeder--> appsrc name=video_src
                           ├─ 最終フレームを保持し、fpsで複製送出
                           └─ 全フレームをプロファイルの w×h にスケール+レターボックス
```

- `VideoSink::spawn_appsrc(w,h,fps)` が pacerスレッドを起動し、paced BGRA (`w*h*4` bytes) をチャネルに送る。feederスレッドが `push_buffer` + PTS付与
- プロファイル変更は配信中不可 (caps不変のため)

### 3.2 音声キャプチャ (WASAPI)

- **列挙:** `IMMDeviceEnumerator::EnumAudioEndpoints(eRender/eCapture)` でデバイス列挙、`IAudioSessionManager2` で per-app セッション列挙
- **キャプチャ:** システム全体=LOOPBACK / per-app(複数)=プロセスループバック / マイク=通常キャプチャ
- **フォーマット:** 全て `48kHz Float32 Stereo` にリサンプルしてからミキシング

### 3.2.3 音声ミキシング (Rust側で完結)

```
[Chrome PCM 48k f32] ─┐
[Spotify PCM 48k f32] ─┼─> Rust Mixer (f32加算 + clamp + ゲイン) ─> audio_rx ─> appsrc name=audio_src
[Mic PCM 48k f32] ─────┘        ↕ VU計算(peak/rms) はここで算出し event でReactへ
```

- **ゲイン/ミュート:** 各ソースに `gain: f32 (0.0-2.0)` と `muted: bool`。`VU` は 50ms ごとに計算
- **複数アプリの加算:** `f32` で加算後 `clamp(-1.0,1.0)`
- `AudioSink::spawn_appsrc(mixer)` が混合済みブロックをチャネルに送る。feederがLE bytes化して `push_buffer`

---

## 4. GStreamer連携設計

### 4.1 方針

- **GStreamerの責務はエンコード + 多重化 + RTMP送信のみ。** キャプチャ・ミキシングはRustが担う
- **供給は `appsrc` 2本** (映像 `BGRA` / 音声 `F32LE 48k stereo`)。`appsrc` は `is-live=true, format=time, do-timestamp=true`
- **bus監視:** `ERROR`/`EOS` をsupervisorスレッドが受け、F-ST-04リトライへ (§9)

### 4.2 パイプライン構成

```
video_src(appsrc) → videoconvert → videoscale → capsfilter(w,h,fps) → queue → <encoder> → h264parse → mux.
audio_src(appsrc) → audioconvert → audioresample → capsfilter(48k/2ch) → queue → <aacenc> → aacparse → mux.
flvmux name=mux streamable=true → rtmp2sink location=rtmp://…/{key}
```

- `<encoder>`: §8.1の解決結果 (例 `nvh264enc`)。`<aacenc>`: `voaacenc` 優先、無ければ `avenc_aac`
- `gst-launch` 等価文字列は `ezstreamer-core::gst::build_launch_string` が生成 (デバッグ/E2E用。アプリは要素APIで構築)

### 4.3 エンコーダプロパティ (Topaz安全・§4.3)

| 要素 | プロパティ |
|---|---|
| NVENC系 | `preset=low-latency rc-mode=cbr bitrate=<v> gop-size=<fps*2> bframes=0` |
| QSV/AMF | `bitrate=<v> gop-size=<fps*2> bframes=0 rate-control=cbr` 系 |
| VAAPI/Vulkan | `bitrate=<v> keyframe-period=<fps*2> bframes=0` |
| x264enc | `bitrate=<v> key-int-max=<fps*2> bframes=0 option-string=sliced-threads=1:sync-lookahead=0:scenecut=0` (`zerolatency`禁止) |

- **GOP:** `fps*2` で2秒固定 (`F-EN-05`)
- **上限ガード:** `F-EN-04` で `v_kbps>2000` or `a_kbps>320` はパイプライン構築前に `Err(OverBitrate)` を返しUIで赤表示
- プロパティは存在確認 (`has_property`) してから設定。ランタイム差異で欠ける物があっても起動する

### 4.4 ランタイム解決

- `ensure_bundled_runtime()`: `resources/gstreamer/{bin,lib/gstreamer-1.0}` があれば `PATH` 先頭 + `GST_PLUGIN_PATH` に設定し、無ければシステムランタイムにフォールバック (開発機)
- `gst::init()` 失敗時は `probe_encoders` が `usable:false` + 理由を返し、UIで赤表示+配信開始無効化

---

## 5. Tauri IPC設計 (ezTopazと同一。`start_portal_picker`のみ削除)

### 5.1 Commands (invoke)

```rust
#[tauri::command] fn get_displays() -> Result<Vec<Display>, String>
#[tauri::command] fn get_windows() -> Result<Vec<WindowInfo>, String>
#[tauri::command] fn get_audio_devices() -> Result<AudioDevices, String>
#[tauri::command] fn get_profiles() -> Result<ProfilesConfig, String>
#[tauri::command] fn save_profiles(cfg: ProfilesConfig) -> Result<(), String>
#[tauri::command] fn probe_encoders(app: AppHandle) -> Result<Vec<EncoderInfo>, String>
#[tauri::command] fn start_stream(cfg: StreamConfig) -> Result<(), String>
#[tauri::command] fn stop_stream() -> Result<(), String>
#[tauri::command] fn start_preview(cfg: StreamConfig) -> Result<(), String>
#[tauri::command] fn stop_preview() -> Result<(), String>
#[tauri::command] fn update_audio_mix(mix: AudioMixUpdate) -> Result<(), String>
#[tauri::command] fn get_status() -> Result<StreamStatus, String>
#[tauri::command] fn get_vu() -> Result<VuMeter, String>
#[tauri::command] fn copy_to_clipboard(text: String) -> Result<(), String>
#[tauri::command] fn open_logs_dir() -> Result<(), String> // explorerを開く
```

### 5.2 Events (listen)

```rust
emit("stream://status", StreamStatus { is_live, duration_sec, bitrate_kbps, dropped_frames })
emit("stream://vu", VuMeter { ... })          // 50ms
emit("stream://preview", PreviewFrame { data_url, w, h }) // 配信前 1fps
emit("stream://log", LogLine { level, msg })  // bus ERROR/EOS要約
emit("stream://error", StreamError { code, msg })
```

### 5.3 型定義 — ezTopazと同一 (`StreamConfig` の `hw_direct`/`direct_input` は旧キーとして読込互換のみ)

---

## 6. React UI設計 (ezTopazと同一。Portalピッカーボタンのみ削除)

```
App
 ├─ Header (ezStreamer | ●00:12 | ja/en | ⚙)
 ├─ Main (tabs: Screen | Audio | Output)
 │   ├─ ScreenSelector: Radio[Display/Window] + DisplayGrid + WindowList + PreviewCanvas(16:9)
 │   ├─ AudioSelector: Radio[System/Apps] + AppMultiSelect(checkbox) + MicSelect + VuMeter * N
 │   ├─ ProfileSelector: [Low][Mid●][High][1080p⚠] + EncoderSelect(auto/...)
 │   └─ StreamControl: IngestInput(editable) + KeyInput + UrlCopy(PC/Quest) + BigToggle
 └─ SettingsModal (Profiles CRUD + EncoderDetails + Logs + Licenses)
```

- 状態管理は `zustand` (個別セレクタのみ。オブジェクトリテラル selector禁止 — 無限再レンダで白画面化するため。`tauri-ui-debug` skill参照)

---

## 7. 設定・永続化 (ezTopazと同一。パスのみWin専用)

- `Win: %APPDATA%/ezStreamer/profiles.json`
- `logs/: %APPDATA%/ezStreamer/logs/ezStreamer-YYYY-MM-DD.log` (bus ERROR/EOS + リトライ記録)
- スキーマは `requirements.md` §8.2。旧キー (`hw_direct`, `direct_input`) は読み捨て
- 保存は atomic write (tmp→rename)

---

## 8. エンコーダ設計

### 8.1 判定ロジック

```rust
// レジストリlookupのみ。子プロセス起動・テストエンコード・キャッシュファイルなし
fn usable() -> Vec<String> { candidates.filter(|id| any element present) }
fn best() -> String { nvenc > qsv > amf > vaapi > libx264 } // vulkanは手動のみ
```

- **自動:** 上記順。`vulkan` は自動では選ばない
- **手動:** UIで `auto` 以外を選んだ場合はレジストリ有無に関わらず要求し、要素不在なら起動時エラー+UI赤表示

---

## 9. エラーハンドリング

| エラー | 検出 | UI表示 |
|---|---|---|
| GStreamer不在 | `gst::init()` 失敗 / 要素不在 | モーダル「GStreamerランタイムが見つかりません」+ 配信開始無効 |
| デバイス未接続 | `start_stream` 前に `selected` が `get_devices` に無い | インライン赤「デバイスが見つかりません」 |
| ビットレート超過 | `cfg.v_kbps>2000` or `a_kbps>320` | インライン赤「Topaz上限を超えています」+ 開始無効 |
| パイプライン起動失敗 | preroll 400ms 以内の bus ERROR / 要素link失敗 | `stream://error` でモーダル + logsリンク |
| 配信切断 | bus ERROR/EOS | `F-ST-04` で3回リトライ(1/2/4s指数バックオフ)、UIで「再接続中 1/3」表示。3回失敗で停止。**再生成時はキャプチャ+パイプライン全体を作り直す** |
| StreamKey空/不正 | `key.len()<3` or 正規表現外 | インライン赤 |

- **ログ:** busメッセージ・リトライ・ feeder異常を `logs/` に追記
- **プロセス後始末:** プロセス内パイプラインのためゾンビなし。`stop` は EOS送信→drain→`Null`。アプリ終了時は `stop_stream` 相当を実行

---

## 10. パフォーマンス・リソース

| 項目 | 目標 | 対策 |
|---|---|---|
| CPU (720p30 x264) | <15% | BGRA→YUV変換は `videoconvert` に任せる |
| CPU (NVENC) | <5% | WGC+GPUエンコードでCPU解放 |
| メモリ | <300MB | フレームはチャネル1本、ログはローテート(10MB) |
| 起動時間 | <2秒 | probeはレジストリlookupのみ (キャッシュ不要) |
| バンドル | NSIS <150MB | GStreamerはallowlist subset同梱 (§13.2) |

---

## 11. セキュリティ・ライセンス

- **画面共有権限:** WGCのOS標準ダイアログ経由のみ
- **StreamKey:** 平文保存だが公開情報(視聴URLの一部)なので暗号化不要。`README` に明記
- **GStreamer:** LGPL。`GSTREAMER-NOTICE.txt` + 取得元URLを `Settings > Licenses` と `README` に記載。libx264/GPL汚染なし (FFmpeg同梱時代の問題が解消)
- **Tauri:** `tauri.conf.json` で `csp: default-src 'self'`、外部通信はRTMPのみ

---

## 12. テスト設計

### 12.1 単体

- `config::tests` — JSON roundtrip, 上限ガード, マイグレーション (旧キー互換)
- `gst::pipeline::tests` — プラン解決, ビットレートガード, launch文字列 (tune禁止表明), GOP
- `gst::probe::tests` — 要素名→UI id対応, 優先順, vulkan手動限定
- `gst::supervisor::tests` — backoff, ランタイム探索順
- `audio::mixer::tests` — f32加算, `clamp`, `gain`, VU計算
- React: `vitest` で `ProfileSelector` の `1080p` 警告表示, `copy` 機能

### 12.2 結合 (Windows実機)

- `capture::screen` — WGCで1フレーム取得できること
- `capture::audio` — 対象アプリの音だけが取得できること (VUで確認)
- `GstStream` — 起動→2秒で `isLive` が `true`、`ffprobe` で `2000k±10%` になること

### 12.3 受入 (requirements.md §10 `AC-01`〜`11`)

| AC | 自動/手動 |
|---|---|
| `AC-01` 未設定で開始不可 | 自動 (vitest) |
| `AC-04/04c/04d` 音声分離 | 手動 (実機 + ヘッドホン) |
| `AC-05` 高画質 2000k | 手動 (ffprobe) |
| `AC-08` Win2004+/11 | 手動 (実機 Topaz Playerで視聴) |
| `AC-11` 素のWindowsで同梱完結 | 手動 (クリーンVM + NSIS) |

---

## 13. ビルド・配布

### 13.1 ローカル (Windows)

```powershell
pnpm i; pnpm tauri dev    # 開発 (GStreamer MSVC Runtime+Development要)
pnpm tauri build          # リリース (resources/gstreamer はCI配置。ローカルは空=システムランタイム使用)
```

### 13.2 CI (GitHub Actions, `windows-latest` のみ)

```yaml
- choco install gstreamer --version <pinned>  # MSVC runtime+dev、GSTREAMER_1_0_ROOT_MSVC_X86_64
- cargo test -p ezstreamer-core
- cargo check -p ezstreamer
- pnpm i && pnpm build && pnpm test
```

Release (NSIS): 同ランタイムから allowlist subset を `src-tauri/resources/gstreamer/` にコピー:

- `bin/`: `gstreamer-1.0-0.dll`, `glib-2.0-0.dll`, `gobject-2.0-0.dll`, `gst*-1.0-0.dll` 系 + 依存 (`intl`, `ffi`, `pcre2`, `z`, `crypto/ssl` 系)
- `lib/gstreamer-1.0/`: `gstapp`, `gstvideoconvertscale`, `gstaudioconvert`, `gstaudioresample`, `gstvideofilter`?, `gstflv`, `gstrtmp2`, `gstx264`, `gstopenh264`, `gstvaapi`?, `gstnvcodec`, `gstqsv`, `gstamfcodec`, `gstd3d11`, `gstwasapi2`
- pin: workflow の `GSTREAMER_URL`/`GSTREAMER_VERSION` を更新 (design §11相当の provenance は `GSTREAMER-NOTICE.txt` にCIが追記)

### 13.3 配布

- GitHub Releases + BOOTH (手動DL)。自動更新なし
- `README` に `Topaz FANBOX` 支援リンクを明記

---

## 14. 実装順序 (WBS 骨子)

1. **スパイク (0.5週):** `appsrc→{nvh264enc,x264enc}→flvmux→rtmp2sink` のPoC (`gst-launch-1.0` + 最小Rust)。Topaz実 ingest への到達確認
2. **基盤 (0.5週):** Tauri+React雛形 (ezTopaz流用), ConfigManager, probe, IPC骨組み ← 本書時点で完了
3. **画面 (0.5週):** ScreenSelector + Preview + 全画面/ウィンドウ切替 ← 継承済み、実機確認のみ
4. **音声 (0.5週):** AudioSelector(複数) + Mixer + VU ← 継承済み、実機確認のみ
5. **配信 (1週):** GstPipeline western調整 (NVENC/QSV/AMF実機プロパティ、レイテンシ)、Ingest可変 + URLコピー + エラーハンドリング
6. **仕上げ (0.5週):** i18n確認、ログ、設定モーダル、AC手動テスト (AC-11含む)、ドキュメント

---

## 15. 未決事項

- 各HWエンコーダ要素のプロパティ名はランタイム差異がある (`nvh264enc` vs `nvenc_h264enc` 等)。`has_property` ガードで起動はするが、最適プロパティの確定はWindows実機スパイク後
- `F-EN-06` (上級者向け詳細引数) をGStreamerでどう露出するか (要素プロパティ直指定欄か、launch断片か)。MVPでは見送り可
- 同梱 subset の最終allowlistはサイズ実測後 (目標 NSIS <150MB)

---

## 16. 変更履歴

| 版 | 日付 | 変更 |
|---|---|---|
| 0.1 | 2026-09-04 | 初版作成 (ezTopaz design v0.2からfork: §4 GStreamer化、§8.1 レジストリprobe、Linux系全削除、§13.2 MSVC同梱、WBS短縮) |
