# ezStreamer コードレビュー報告 (全体 + GStreamer実装検証)

- **日付**: 2026-09-10
- **対象**: `main` @ `cc16996` (作業開始時点、全ファイル静的検査)
- **検証環境**: Linux (実機テスト実行: core / フロントエンド)
- **テスト実行結果**:
  - `cargo test -p ezstreamer-core` → **61/61 OK**
  - `pnpm test` (vitest) → **14/14 OK**
  - `cargo clippy -p ezstreamer-core` → 軽微な指摘 8 件 (後述)
- **GStreamer実装の検証方法**:
  - 公式ドキュメント (gstreamer.freedesktop.org) で該当要素のプロパティ/enumニック/pad caps を実地照合
    (`nvh264enc`, `qsvh264enc`, `amfh264enc`, `vah264enc`, `vulkanh264enc`, `appsrc/GstAppLeakyType`)
  - `gstreamer-rs 0.23.7` / `glib 0.20.12` の vendored ソースで `set_property_from_str` の panic 条件を確認
  - `pipewire-rs 0.10.1` / `libspa 0.10.1` の vendored ソースで buffer API (`Data::chunk().stride()`) を確認

---

# 総評

| 領域 | 評価 |
|---|---|
| アーキテクチャ (core=純データ / backend=GStreamer実行) | ✅ 良い。CIがGStreamerなしで回る設計は正解 |
| GStreamerパイプライン構築 (`gst_stream.rs`) | ⚠️ トポロジ・appsrc設定は正しいが、**エンコーダプロパティ適用がpanicクラッシュの温床** |
| キャプチャ (WGC/WASAPI) | ✅ 概ね堅実。staging texture毎フレーム生成など軽微な非効率 |
| キャプチャ (Portal/PipeWire) | ⚠️ **stride無視の潜在バグ**、RTスレッド上でのロック/確保がRT安全違反 |
| ライフサイクル / F-ST-04リトライ | ⚠️ stopとリトライ起動の競合が1件 |
| フロントエンド (React/zustand) | ✅ 良い。i18nキー不足なし、状態管理は健全 |
| CI / バンドル (DELAYLOAD設計) | ✅ 良い |

実装は設計ドキュメント (design.md §3〜§9, §13) と高い整合性があり、コメントに「なぜそうしたか」の根拠 (検証済みcaps、回帰バグの理由) が書かれている点は優れている。一方で、**エンコーダプロパティの「値の妥当性」を検証しない `has_property` ガード運用**が今回の重大バグの直接原因となっており、このパターンの構造的改善が最重要。

---

# 🔴 Critical: NVENC `preset=high-performance` は存在しないニック → start_stream でアプリ全体がクラッシュ

**箇所**:
- プロパティ定義: `ezstreamer-core/src/gst/pipeline.rs:97-109` (`EncoderSpec::Nvenc.gst_props`)
- 適用処理: `src-tauri/src/ipc/gst_stream.rs:224-228` (`has_property` → `set_property_from_str`)

**根拠** (3点、いずれも実地確認):

1. **公式ドキュメントの `GstNvEncoderPreset` メンバー一覧**:
   `default / hp / hq / low-latency / low-latency-hq / low-latency-hp / lossless / lossless-hp / p1〜p7`
   — **`high-performance` というニックは存在しない** (p1〜p7 は Since 1.22、`tune` は Since 1.24。1.28 にも追加されていない)。
2. **`set_property_from_str` は値が解決できないと panic する** — `gstreamer-0.23.7/src/gobject.rs`:
   ```rust
   glib::Value::deserialize_with_pspec(value, &pspec).unwrap_or_else(|_| {
       panic!("property '{}' of type '{}' can't be set from string '{}'", ...)
   })
   ```
   `has_property` はプロパティの**存在**のみ見る。値のニックが無効なら、ガードを通過した直後に panic。
3. **panic の発生場所が危険**: `spawn_pipeline` は `start_stream` (sync command) から呼ばれ、
   Tauri v2 の公式仕様「Commands without the async keyword are executed on the main thread」により、
   **panic = プロセス全体のクラッシュ**。コード自身のコメント (gst_stream.rs:236-237) も
   「aborts the Tauri main thread (cannot unwind)」と認識済み。

**影響**: NVIDIA GPU を持つ環境 (ターゲットユーザーの大多数。auto は nvenc を最優先) で
「配信開始」を押すと、OSのクラッシュとともにアプリが落ちる。
`docs/impl-report.md` 残タスク#3の想定「`has_property` ガードのため起動はする」は誤り
(ガードは「プロパティ不在」しか防御しない。「プロパティが存在するが値のニックが無効」を通す)。

**修正案**:

- 要件 (requirements.md §2.2「Max Performance / Look-ahead OFF / Psycho Visual OFF / B-frames 0」) への対応は:
  - `preset=p1` + `tune=ultra-low-latency` (p1=最速, tune は Since 1.24、pinned ランタイム 1.28.6 で利用可)
  - または旧ニック `preset=hp` (High Performance, deprecated)
  - を `gst_props_for` の方言解決に追加 (ランタイム差異は has_property でスキップ、ただし値は必ず実在ニックのみ)
- **構造的修正**: プロパティ適用を panic-safe にする。例:
  ```rust
  for (k, v) in plan.encoder.gst_props_for(&encoder_name, &profile) {
      if let Some(pspec) = encoder.find_property(&k) {
          // 値を事前に deserialize してから set (失敗時はスキップ+ログ、panicしない)
          match encoder.try_set_from_str(&k, &v) { /* 実装例は本文参照 */ }
      }
  }
  ```
  `set_property_from_str` の代わりに `find_property` → `deserialize_with_pspec` → `set_property` の
  3段構成にするか、適用全体を `std::panic::catch_unwind` で包む。
  **本質は「ニック検証を has_property に代えて値レベルで行う」こと**。

---

# 🟠 High: Linux PipeWire 映像で stride を無視 (画像歪みの潜在バグ)

**箇所**: `src-tauri/src/capture/linux/screen.rs:261-262`

```rust
if let Some(bytes) = data.data() {
    let frame = scale_bgra(bytes, w, h, ud.dst_w, ud.dst_h);
```

`scale_bgra` は stride == `w*4` を仮定する。PipeWire/portal screencast は stride に
パディングを入れることがあり、その場合フレームが対角線状にズレて表示される。

**対比**: Windows WGC 側は `RowPitch` を正しく処理している (`windows/screen.rs:223-234`)。

**修正案**: pipewire-rs 0.10.1 には `data.chunk().stride()` がある
(`libspa-0.10.1/src/buffer/mod.rs:159`)。stride を使った行コピーに修正し、
可能なら `chunk.size()` (実データ長) と併用して妥当性チェックを入れる。

---

# 🟠 High: 全音声ソース死亡時に flvmux が停止し、映像ごと固まる (ERROR も出ない)

**箇所**: `ezstreamer-core/src/audio/sink.rs:129-131`

```rust
if available.is_empty() {
    continue;   // 何も emit しない
}
```

flvmux は collect-pads 型で**全パッドにデータが届くまで多重化しない**。
配信中に system ループバックとマイクの両スレッドが死んだ場合
(それぞれ `stream://error` をemitして終了する)、音声 `appsrc` にデータが来なくなり:

- flvmux が待ち続け → vqueue が詰まり → videoconvert が詰まり → appsrc (leaky) が古いフレームを捨てる
- パイプラインは ERROR も EOS も出さない → **F-ST-04 リトライは発火しない**
- UI は「LIVE 表示のまま永久に固まる」

**修正案**: `spawn_mixer` が入力ゼロ時も BLOCK (960サンプル ≈10ms) 分の無音を emit し続ける。
音声ソースなしの起動は `start_audio` が事前にエラーで弾いているため、
無音パディングは安全で、mux ストールを構造的に排除できる。

---

# 🟡 Medium: `spawn_retry_thread` と `stop_stream` の競合で「見えないライブ配信」が残る

**箇所**: `src-tauri/src/ipc/commands.rs:476-499`

リトライスレッドの流れ: `retrying.is_none()` チェック → backoff待ち → `launch_pipeline`
(数百ms以上かかる: キャプチャ+WASAPI+gst::init+パイプライン構築) → `*state.stream = Some(proc)`。

この launch 中に `stop_stream` が実行されると:

1. `stop_stream`: retrying=None 化、既存 stream 停止、capture 後片付け、session クリア
2. その**後**にリトライスレッドの launch が完了し、新しい配信を state にインストール

→ UI は「停止済み」表示のまま、**裏で RTMP へのライブ送信とキャプチャが継続する**
(次回 stop_stream / アプリ終了まで止まらない)。

**修正案**: launch 成功後に再チェックを1行追加:
```rust
Ok(proc) => {
    if state.retrying.lock().unwrap().is_none() {
        proc.stop();               // ユーザー停止が先行していた
        stop_capture_backends(&state);
        return;
    }
    *state.stream.lock().unwrap() = Some(proc);
    ...
}
```

---

# 🟡 Medium: プロファイル入力値の検証不足 → 「リトライ3回→エラー」の遠回りな失敗

**箇所**: `save_profiles` (`commands.rs:138-150`) はビットレートのみ検証。
`SettingsModal.tsx` の `NumberCell` は **fps=0 / 負数 / 奇数w/h を許す**。

- `fps=0` → `Profile::gop() = 0` → `x264enc key-int-max=0` (無限GOP) かつ appsrc caps `framerate=0/1` → 交渉失敗
- 奇数幅 → videoconvert→NV12/I420 変換で not-negotiated
- HWエンコーダ下限 (公式 caps): nvenc `[160,4096]×[64,4096]`、amf `[128,4096]×[128,4096]`、qsv `[16,8192]×[16,8192]`

結果は「起動→bus ERROR→リトライ3回→諦め」の遠回りなエラー体験。

**修正案**:
- `build_plan` (または `save_profiles`) で `fps >= 1`、`w/h 偶数`、エンコーダ共通下限以上を検証し
  `Error::Config` を返す (build_plan は既に bitrate/key/ingest を検証しているので一貫する)
- `NumberCell` に `min` 属性を追加

---

# 🟢 Low / 軽微

| # | 箇所 | 内容 |
|---|---|---|
| 1 | `gst_stream.rs:233-239` | 音声 `bitrate` の string 経由 set は gint/guint 差の回避として正しい ✓。プロパティ適用全体の panic-safe 化を Critical と合わせて実施 |
| 2 | `pipeline.rs:72` | `nvenc_h264enc` は実在しないファクトリ名 (find 失敗でスキップ、実害なし)。削除候補 |
| 3 | `commands.rs:305-321` | `encoder_override` が空文字のとき usable リストを auto 同様に作るが、`build_plan` には `""` がそのまま渡り `EncoderNotAvailable("")` になる非対称。空なら `"auto"` に正規化 |
| 4 | `gst_stream.rs:288-334` | 映像feeder spawn 成功→音声feeder spawn 失敗のとき、映像feederが `wait_for_playing` で永久待機 (stop フラグ未設定)。feeder spawn より前に失敗した場合は全経路で `stop.store(true)` を実行 |
| 5 | `linux/screen.rs:247-270` | PW の process コールバック (RT スレッド) 内で `scale_bgra` (アロケーション + `thread::scope` スレッド生成) と `Mutex::lock` (preview_slot)。`RT_PROCESS` 指定下では XRUN/優先度逆転の温床。ロックフリースロット化し、重い変換は別スレッドへ (Windows 側 preview_last パターンに揃える) |
| 6 | `video/sink.rs` / `gst_stream.rs` | フレーム1枚あたり BGRA コピーが3回 (`scale_bgra` 生成 → `pacer.push` コピー → `out_tx.send(frame.to_vec())`)。`to_vec()` を廃止し所有権移動に |
| 7 | `windows/screen.rs:212-216` | WGC で staging texture を毎フレーム `CreateTexture2D`。セッション開始時に1回作って再利用 |
| 8 | `src-tauri/Cargo.toml:54` | `gstreamer-video` が未使用 (gstvideo DLL へのリンクと DELAYLOAD エントリだけが残る)。削除または使用方針の明示 |
| 9 | `commands.rs:592-609` | `update_audio_mix` がアプリの選択解除を反映しない (insert のみ)。mixer.auto_register が再登録するため事実上無害だが、意図を明示するか remove を実装 |
| 10 | `store.ts:382-400` | start 成功時に `backendError` をクリアしていない。過去の失敗文言が停止後に再表示される |
| 11 | `gst_stream.rs:67` | UI のビットレート表示が開始からの平均値。直近数秒の移動平均 (probe でリングバッファ) の方が配信状態として直感的 |
| 12 | clippy (core) | `div_ceil` (`video/sink.rs:231`)、`useless vec!` ×2、`field assignment outside initializer` ×3、derivable `impl` ×1、`too_many_arguments` ×1 — impl-report 残タスク#5 どおり別 chore PR |
| 13 | `docs/impl-report.md` §3 | core テスト数の記述 58 → 現在 61 |

---

# ✅ 正しいと検証できた GStreamer 実装 (根拠付き)

1. **appsrc 設定** (`gst_stream.rs:196-207`): `is-live=true` + `format=time` + `do-timestamp=true` +
   `max-buffers=4/50` + `leaky-type=Downstream`。
   公式ドキュメントで `GST_APP_LEAKY_TYPE_DOWNSTREAM` = "Leaky on downstream (old buffers)" を確認 —
   **古いフレームを捨てる方向で正しい**。設計 §4.1 (固定増分PTS廃止) の理由付けも技術的に正しい。
2. **tail converter が「冗長ではなく必須」**という説明 (`gst_stream.rs:14-19`):
   リンク時の live caps チェックという理由は正しい。`vah264enc` / `qsvh264enc` / `amfh264enc` の
   sink caps は NV12 のみ (公式 caps で確認) で、BGRA のままのリンク失敗を正確に回避。
   なお `nvh264enc` は sink が BGRA を直接受けるため NVENC 時は tail videoconvert がパススルーとなり無駄がない。
3. **`vulkanupload` の挿入位置** (tail videoconvert の後): `vulkanh264enc` sink は
   `video/x-raw(memory:VulkanImage),format=NV12` のみ (公式 caps 確認) で位置は正解。
4. **AAC 側**: `voaacenc`/`avenc_aac`/`mfaacenc` の `bitrate` (bit/s) は妥当。
   string 経由の set で gint/guint 差による panic を回避する工夫は妥当。
   `aacparse → flvmux` は RTMP の公式パターン通り。
5. **エンコーダプロパティの方言対応** (`pipeline.rs:168-183`):
   - `openh264enc`: bit/s 単位 + `gop-size` ✓
   - `vaapih264enc` (1.26 で削除): `keyframe-period` / `bframes` ✓
   - QSV: `b-frames` / `rate-control=cbr` / `gop-size` ✓ (公式ドキュメント照合済み)
   - AMF: 同上 ✓ (同上)
   - VA: `key-int-max` / `rate-control=cbr` / `b-frames` ✓ (同上)
   - Vulkan: `rate-control=cbr`、デフォルト cqp の指摘も正 ✓ (同上)
   - **問題は NVENC preset のみ** (Critical 参照)。
6. **mux src pad probe でエンコード後バイト数** (`gst_stream.rs:272-279`): F-ST-03 の意図通り。
7. **Windows 同梱ランタイム**: `build.rs` の DELAYLOAD + `gst_preload.rs` の
   AddDllDirectory/LoadLibraryW 依存順プリロード + `ensure_bundled_runtime` の PATH/GST_PLUGIN_PATH。
   preview.01 の「DLL not found」問題への対処として構成は正しい。
   レイアウト2種 (`gstreamer/` と `resources/gstreamer/`) の両検出もテスト付き ✓。
8. **bus supervisor**: ERROR/EOS で終了、ユーザー停止は EOS 送信→3秒で強制 (F-ST-04)。
   `set_state(Null)` を監視スレッドで行い feeder を stop フラグで解放する流れは、
   逆順 (feeder 先に EOF → パイプライン自然終了) より確実で良い設計。
9. **音声 caps の `layout=interleaved`** (`gst_stream.rs:458-465`): layout 無しで
   `audioconvert/audioresample` が set_caps を拒否するという回帰修正は正しい (テスト付き)。
10. **`flvmux streamable=true → rtmp2sink`**: ライブ RTMP の正統な構成。
    rtmp2sink の backpressure は vqueue/aqueue → appsrc (leaky) で最終的に捨てられ、
    デザイン通りのバックプレッシャ連鎖になっている。

---

# 対応の優先順位 (提案)

1. **即修正**: NVENC preset (ニック訂正 + プロパティ適用の panic-safe 化) — 実機 E2E の前に必須
2. **実機 E2E 前までに**: PipeWire stride 修正、音声全停止時の無音 emit
3. **次の PR**: retry/stop 競合、プロファイル入力検証、feeder オーファン、clippy 整形
4. **余裕時**: パフォーマンス系 (コピー削減、staging 再利用、PW RT スレッドの無ロック化)
