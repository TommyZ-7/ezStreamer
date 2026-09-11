# ezStreamer 要件定義書 v0.3

> 作成日: 2026-09-04 | 更新日: 2026-09-11 | 対象: 要件定義フェーズ | ステータス: Draft | リポジトリ: `ezStreamer`
> 派生元: `ezTopaz` 要件定義書 v0.3.2。v0.1時点の差分は **Windows専用化 (§4, §6)** と **FFmpeg→GStreamer置換 (§4.4)** のみ。それ以外の機能要件は同一 (v0.2でLinux/Flatpakを追加)。
> v0.2 (2026-09-10): **Linux/Flatpak 対応をスコープに追加** (元は非スコープ)。実装レビュー修正
> (NVENC preset 要件準拠、設定永続化、ファイルログ、A/V同期、appsrc backpressure、マイクデバイス選択、
> per-app 音量UI、カーソルON/OFF、プロファイル export/import) を反映。
> v0.3 (2026-09-11): **GUI を Tauri + React から Rust + egui/eframe へ移行** (§4.2)。UI要件に
> 「直線的でつながりのあるデザイン」「絵文字・グラデーション不使用」を追加 (§7)。機能要件は変更なし。

---

## 1. はじめに

### 1.1 背景
VRChat内で画面共有を行う際、デファクトは `TopazChat` である。現状は `OBS Studio` で `rtmp://topaz.chat/live` へRTMP配信する運用だが、OBSはYouTube/Twitch等の汎用配信ソフトであり、TopazChat配信のたびに「サービス=カスタム」「サーバー/ストリームキー切替」「出力/映像設定の切替」が発生し運用負荷が高い。

### 1.2 目的
TopazChatへの映像・音声配信に特化し、「起動→画面/音声選択→配信開始→URLコピー」で完結する軽量配信ソフト `ezStreamer` (ezTopaz後継) を提供する。OBSの汎用性を捨てる代わりにTopazChatの制約・推奨設定をデフォルト化し、設定迷子をゼロにする。

### 1.3 スコープ

- **含む**: 画面キャプチャ(全画面/ウィンドウ)、音声キャプチャ(システム/アプリ指定(複数選択・含める方式)/マイク)、エンコード・RTMP送信、プロファイル管理(低/中/高 + 1080p警告付)、URLコピー、Ingest URL可変(MVPから対応)、エンコーダ手動選択、日英対応
- **含む (v0.2追加)**: Linux (Flatpak / Wayland) 対応。画面は xdg-desktop-portal ScreenCast、音声は PipeWire、配布は Flatpak (GNOME runtime)
- **含まない(今回)**: 録画機能(ローカル保存)、シーン合成(複数ソースのレイアウト)、仮想カメラ、クラウド機能、自動更新、テレメトリ
- **非スコープ (ezTopazからの削減)**: X11/Wayland/Portal/PipeWire の自前実装 (LinuxはPortal/PipeWire経由で対応)、FFmpeg sidecar・named pipe、AppImage/deb/AUR、ddagrab direct入力

### 1.4 用語

| 用語 | 説明 |
|---|---|
| TopazChat | よしたか氏(@tyounanmoti)運営のVRChat向け低遅延配信サービス。個人利用無償 |
| TopazChat Player | ワールドに配置する受信用ギミック。BOOTH配布(https://booth.pm/ja/items/1752066) |
| Ingest URL | OBS等が映像を送る先。デフォ `rtmp://topaz.chat/live` (本ソフトではMVPから編集可) |
| Playback URL | VRChat VideoPlayerに入力する視聴URL。PC: `rtspt://topaz.chat/live/{key}` / Quest: `rtsp://topaz.chat/live/{key}` |
| StreamKey | 配信を識別する英数字。衝突すると他人の配信と混線。URLの一部であり秘密鍵ではない |
| AAC 320kbps / 2Mbps | TopazChatの上限。超過で強制切断 |

---

## 2. TopazChat 詳細調査サマリ (ezTopaz継承)

### 2.1 アーキテクチャ
```
[配信者PC: ezStreamer] --RTMP--> [topaz.chat:1935/live/{key}] --RTSP/RTSPT--> [VRChat AVPro Player]
```
- Ingest: `RTMP` (`rtmp://topaz.chat/live`, FLVコンテナ, H.264 + AAC)
- Playback: `rtspt://` (TCP interleaved RTSP, PC低遅延) / `rtsp://` (Quest)

### 2.2 制約・推奨値 (公式+Booth引用)

| 項目 | 値 | 備考 |
|---|---|---|
| 映像ビットレート上限 | **2000 kbps 以下** (厳守) | 超過で強制切断 |
| 音声ビットレート上限 | **320 kbps 以下** (AAC Stereo推奨) | 320kbpsが最高 |
| フレームレート | **60fps 推奨** | 視聴側が30fps下回ると映像が大きく崩れる注意あり |
| エンコーダNG例 | x264 `zerolatency` tune, NVENC `Low Latency` preset | 使用時VRChat側で灰色画面になることがある。本ソフトでは使用しない |
| NVENC推奨(Booth) | `NVENC / Max Performance / Profile High / Look-ahead OFF / Psycho Visual OFF / Max B-frames 0` | 低遅延最優先チューニング |

---

## 3. ステークホルダー

| 区分 | 例 |
|---|---|
| 配信者 | VRChatイベント主催/演者/VJ/DJ、画面共有したい一般ユーザ |
| 視聴者 | VRChatワールド参加者(PC/Quest) ※本ソフトの直接ユーザではないがURL配布先 |
| ワールド作者 | TopazChat Player設置者、StreamKeyを発行/周知する人 |
| 運営 | 本ソフト開発・配布者 |

---

## 4. 開発言語・技術スタック選定

### 4.1 要求
- Windows 10 2004+ / 11、および Linux (Flatpak / Wayland + PipeWire)
- モダン・軽量 (OBS 200MB+ / Electron 150MB+ は避ける)
- 画面/ウィンドウ列挙、音声デバイス列挙(アプリ別含む)、RTMP配信、エンコーダ制御 (HW accel) が可能
- プロセス内でエンコード〜RTMP送信を完結 (sidecarプロセス・名前付きパイプを使わない)
- オフライン動作、MIT公開

### 4.2 採用: **Rust + egui/eframe + GStreamer (gstreamer-rs)**

**理由:**
1. 軽量: WebView / Node ツールチェーンを持たない。単一のネイティブバイナリ (egui/eframe + glow) で描画
2. プロセス内完結: GStreamerパイプライン (`appsrc → encode → flvmux → rtmp2sink`) により、FFmpeg sidecar・名前付きパイプ・stderrパースが不要。起動高速化・クラッシュ時のゾンビプロセス問題の解消
3. Native: 画面は `WGC` (Windows) / Portal ScreenCast (Linux)、音声は `WASAPI` (Windows) / PipeWire (Linux) をRustで直接取得し、`appsrc` に供給
4. Windows + Linux 対応: WindowsはWGC/WASAPI、Linuxは xdg-desktop-portal (ashpd) + PipeWire。Mixer/FramePacer/GStreamer/UI/config はプラットフォーム非依存
5. UI要件: 直線的でつながりのあるレイアウト、絵文字・グラデーション不使用 (§7)

**アーキテクチャ案:**
```
[egui UI (ja/en, native)] <-> [backend worker (Rust) commands/events]
                              [Capture Manager (WGC/WASAPI or Portal/PipeWire)] -> appsrc
                              [AudioMixer/FramePacer (Rust)] -> appsrc
                              [GstPipeline: encode(H.264) + flvmux + rtmp2sink]
                              [Config (profiles.json + ingestUrl)]
```

**GStreamer同梱方針 (Windows):** 公式 GStreamer MSVC 64-bit ランタイムのサブセットを
インストーラに同梱し、同梱パス (`resources/gstreamer/`) を `PATH` + `GST_PLUGIN_PATH` に設定して
初期化する (完全オフライン要件のため初回DLなし)。起動時にレジストリでHWエンコーダ存在を確認し
自動選択。手動オーバーライドも設定画面で可能。**Linux (Flatpak):** GNOME runtime が
GStreamer + PipeWire + GL を提供するため同梱しない。

### 4.3 エンコーダ対応表

| UI id | GStreamer要素 (先勝ち) | 備考 |
|---|---|---|
| `h264_nvenc` | `nvh264enc`, `nvenc_h264enc` | 自動判定の最優先 |
| `h264_qsv` | `qsvh264enc` | |
| `h264_amf` | `amfh264enc` | |
| `h264_vaapi` | `vah264enc`, `vaapih264enc` | Windowsでは通常不可 (UI parityのため残す) |
| `h264_vulkan` | `vulkanh264enc` | 手動選択のみ (自動では選ばない) |
| `libx264` | `x264enc`, `openh264enc` | ソフトウェアフォールバック (要素が存在する場合のみ usable。openh264enc は bitrate bit/s) |

共通チューニング (Topaz安全): B-frames 0 / GOP 2秒 / CBR / High profile。x264系に `zerolatency` tuneは使わない (灰色画面不具合)。NVENC は `preset=hp` (High Performance) を使い、`Low Latency` preset は禁止 (灰色画面不具合)。`vulkan` は手動選択のみ。

---

## 5. 機能要件 (ezTopazと同一)

### 5.1 画面キャプチャ (MUST)

| ID | 要件 | 詳細 | 優先度 |
|---|---|---|---|
| F-SC-01 | 全画面キャプチャ | マルチモニタ時はモニタ選択UIを表示。解像度/FPSはプロファイルに従う | Must |
| F-SC-02 | ウィンドウキャプチャ | 起動中ウィンドウ一覧を列挙、選択 (WGC `CreateForWindow`) | Must |
| F-SC-03 | プレビュー | 配信前にローカルプレビュー(サムネ更新 1fps) | Must |
| F-SC-04 | カーソル表示 | カーソル表示ON/OFF (初期ON) | Should |

**実装方針 (Linux):** 画面/ウィンドウは `xdg-desktop-portal` ScreenCast のOSピッカーで選択し、
PipeWire のリモートfdを直接受けて BGRA フレーム化する。アプリ側のウィンドウ列挙は行わない。
カーソルは Portal の `CursorMode` (Embedded/Hidden) で指定 (初期ON)。

### 5.2 音声キャプチャ (MUST)

| ID | 要件 | 詳細 | 優先度 |
|---|---|---|---|
| F-AU-01 | システム音声 | OS全体の再生音(Desktop Audio)をキャプチャ。`F-AU-02a`と排他 | Must |
| F-AU-02a | アプリ指定(含める, 複数可) | **1つ以上のアプリ**の音声のみをキャプチャ。システム音声と排他、複数選択可 | Must |
| F-AU-03 | マイク入力 | 入力デバイス列挙・選択、**オンオフ(☑)・ミュート・ゲイン** | Must |
| F-AU-04 | ミキシング | 上記をミックスしてAAC 1本に。UIで各ソースのVU/ミュート/音量スライダー。複数アプリはRust側で合成しGStreamerへ1本のPCM | Must |
| F-AU-05 | デバイス永続化 | 選択デバイスをプロファイルに保存、未接続時は警告 | Must |
| F-AU-06 | サンプルレート | 48kHz固定、自動リサンプル | Must |

**実装方針:** Windows は `WASAPI` でプロセス別セッション列挙し、プロセスループバックAPI (Win10 2004+) で取得。Linux は PipeWire で system=既定sinkのmonitor / アプリ=対象node / マイク=Audio/Source node を取得。マイクは選択デバイスを開き、不在時は開始時にエラー表示 (F-AU-05)。合成はRust側で実施し `appsrc` (F32LE 48kHz stereo) へ。

### 5.3 エンコーダ・プロファイル (MUST)

| ID | 要件 | 詳細 | 優先度 |
|---|---|---|---|
| F-EN-01 | プリセット切替 | **低/中/高 + 1080p** の4ボタンを常時表示 | Must |
| F-EN-02 | カスタムプロファイル | 追加/複製/削除/JSON編集可。保存先 `profiles.json` | Must |
| F-EN-03 | 自動HW判定+手動選択 | `h264_nvenc > h264_qsv > h264_amf > h264_vaapi > libx264` で自動判定 (`vulkan`は手動のみ)。設定画面で手動オーバーライド可 | Must |
| F-EN-04 | 上限ガード | 映像>2000k / 音声>320k は保存時にエラー/クランプ、UIで赤表示 | Must |
| F-EN-05 | キーフレーム | GOP=2秒固定 | Must |
| F-EN-06 | 詳細引数 | 上級者向けに追記テキスト欄 (GStreamer要素プロパティ相当。MVPでは見送り可) | Should |

**デフォルトプロファイル (Topaz上限内で設計):**

| プロファイル | 解像度 | FPS | 映像kbps | 音声kbps | エンコーダ想定 | UI表示 |
|---|---|---|---|---|---|---|
| 低画質 (Low) | 854x480 | 30 | 800 | 128 | x264 | [低] |
| 中画質 (Mid) **初期選択** | 1280x720 | 30 | 1500 | 192 | NVENC / x264 | [中●] |
| 高画質 (High) | 1280x720 | 60 | 2000 | 320 | NVENC / x264 | [高] |
| 1080p (警告付) | 1920x1080 | 30 | 2000 | 320 | NVENC | [1080p ⚠] |

### 5.4 配信制御 (MUST)

| ID | 要件 | 詳細 | 優先度 |
|---|---|---|---|
| F-ST-01 | StreamKey入力 | 3-64文字、英数字/ハイフン/アンダースコア。空欄エラー、汎用キー警告 | Must |
| F-ST-02 | 配信開始/停止 | 大トグル1つ (灰:停止中 / 赤:配信中) | Must |
| F-ST-03 | 状態表示 | 配信時間、推定ビットレート、ドロップ、VU | Must |
| F-ST-04 | 自動再接続 | 切断時3回リトライ(指数バックオフ1/2/4s。パイプライン全体を再生成) | Should |
| F-ST-05 | Ingest URL可変 | デフォ `rtmp://topaz.chat/live` を表示、**編集可** | Must |

### 5.5 ストリームURLコピー (MUST)

| ID | 要件 | 詳細 | 優先度 |
|---|---|---|---|
| F-URL-01 | ワンクリックコピー | Key入力と同時に `rtspt://topaz.chat/live/{key}` 生成→コピー | Must |
| F-URL-02 | 2種表示 | `rtspt://` (PC) と `rtsp://` (Quest) 各々コピー可 | Must |
| F-URL-04 | コピー通知 | 成功時トースト | Must |

### 5.6 設定・永続化

| ID | 要件 | 詳細 |
|---|---|---|
| F-CF-01 | プロファイル保存 | `profiles.json` (Win `%APPDATA%/ezStreamer/`)。`hw_direct`/`direct_input` は旧キーとして読込互換のみ維持 |
| F-CF-02 | 前回値復元 | 起動時に前回の画面/音声/プロファイル/StreamKey/IngestURLを復元 |
| F-CF-03 | エクスポート/インポート | プロファイルJSON入出力 |
| F-CF-04 | ログ | 配信ログを `logs/` に保存、UIから「ログを開く」 |
| F-CF-05 | 言語 | 日英切替。`ja`/`en` リソース、初期はOS言語自動選択 |

---

## 6. 非機能要件

| 区分 | 要件 | 目標値 |
|---|---|---|
| 性能 | CPU (720p30, x264) | < 15% / NVENC時 < 5% |
|  | メモリ | < 300MB |
|  | 起動時間 | < 2秒 (レジストリprobeはµsオーダーのためキャッシュ不要) |
|  | バンドルサイズ | NSIS < 150MB (GStreamer同梱込み) / Flatpak 数MB (GNOME runtime利用) |
| 互換 | Windows | 10 2004+ / 11 |
|  | Linux | Wayland + xdg-desktop-portal + PipeWire (Flatpak / GNOME runtime) |
| 信頼性 | 配信継続 | 瞬断で自動復帰、クラッシュ時パイプライン確実停止 |
| 保守性 | ログ | GStreamer bus ERROR/EOS 保存、UIに要約 |
| セキュリティ | 権限 | 画面共有はWGC標準ダイアログ経由。Keyは平文保存(公開情報)明記 |
| 配布 | インストーラ | Win: NSIS / Linux: Flatpak。**自動更新なし、手動DL** |
|  | ライセンス | **MIT**でGitHub公開。本体MIT + 同梱GStreamer LGPL (GPL汚染なし) |
| 国際化 | 日英両対応 | MVPから `ja`/`en` 完全対応 |
| プライバシ | テレメトリ | **取得しない**、完全オフライン |
| 録画 | ローカル録画 | **なし** (配信専用) |

---

## 7. 画面遷移・UI要件 (v0.3: egui)

**デザイン要件 (v0.3追加):**
- **直線的でつながりのあるデザイン:** 1pxの罫線と直角で区切り、セクションを連結する。
  浮遊カード・影・角丸は使用しない。
- **絵文字・グラデーション禁止:** アイコンは図形とテキストのみ。VUメーター等は
  離散ブロックのハードステップで描画する。
- 配色は無彩色 + アクセント1色。赤=配信中、アンバー=警告、緑=正常のみ機能色。

```
┌──────────────────────────────────────────────────────┐
│ ezStreamer │ ■配信中 00:12  1500 kbps │ ja/en │ 設定  │
├──────────┬───────────────────────────────────────────┤
│ 01 画面 ─┤ 画面: 全画面/ウィンドウ + プレビュー16:9    │
│ 02 音声 ─┤ 音声: システム/アプリ + mic + VU          │
│ 03 出力 ─┤ 出力: [低][中][高][1080p] + エンコーダ      │
│          │                                            │
├──────────┴───────────────────────────────────────────┤
│ Ingest [rtmp://topaz.chat/live]  Key [my-key____]     │
│ PC    rtspt://...            [コピー]                 │
│ Quest rtsp://...             [コピー]                 │
│                                  [ 配信開始 ] 大ボタン │
└──────────────────────────────────────────────────────┘
```

- **原則:** OBSの「シーン/ソース」概念なし。チェックボックスとドロップダウンで完結。
- **配信開始ボタンは常時下部固定 (StreamDock)**。**VUメーター必須**。
- **エラー表示:** ビットレート超過、デバイス未接続、Key空欄、GStreamer不在は
  インライン赤表示 + 配信開始無効化 (失敗はトーストでも通知)。
- **言語切替:** ヘッダの `ja/en` トグルで即時切替。
- **設定画面:** 全画面ペイン (モーダルではない)。「戻る」で本体へ。

### 7.2 設定画面 (全画面ペイン)
- プロファイル一覧(低/中/高/1080p+カスタム) 編集・複製・削除
- エンコーダ: 自動結果表示 + 手動選択 (auto / libx264 / nvenc / qsv / amf / vaapi / vulkan)
- JSONエクスポート (設定ディレクトリの `exports/`) / インポート (パス入力 or ドラッグ&ドロップ)
- ログ/バージョン/ライセンス(MIT + GStreamer LGPL)

---

## 8. 外部IF・データ設計

### 8.1 外部IF
| 相手 | プロトコル | 方向 |
|---|---|---|
| topaz.chat:1935 (デフォ) / 任意Ingest | RTMP/FLV (H.264+AAC) | 送信のみ |
| OS画面API | WGC(Win) | 取得 |
| OS音声API | WASAPI(Win) | 取得 |
| クリップボード | OS | 書込 |

### 8.2 設定ファイル例

```json
// profiles.json
{
  "version": 2,
  "locale": "ja",
  "ingestUrl": "rtmp://topaz.chat/live",
  "activeProfile": "mid",
  "profiles": {
    "low":  { "name":"低画質", "w":854, "h":480, "fps":30, "v_kbps":800,  "a_kbps":128, "encoder":"auto" },
    "mid":  { "name":"中画質", "w":1280,"h":720, "fps":30, "v_kbps":1500, "a_kbps":192, "encoder":"auto" },
    "high": { "name":"高画質", "w":1280,"h":720, "fps":60, "v_kbps":2000, "a_kbps":320, "encoder":"auto" },
    "1080p":{ "name":"1080p", "w":1920,"h":1080,"fps":30, "v_kbps":2000, "a_kbps":320, "encoder":"auto", "warn":"Topaz上限2000kのため720p推奨" }
  },
  "lastStreamKey": "my-key",
  "lastSources": { "screen":"monitor:0", "includeApps":["Chrome","Spotify"], "mic":{"device":"default","enabled":true} },
  "encoderOverride": "auto"
}
```

---

## 9. 制約・前提

- TopazChatは個人運営・試験運用。**映像配信が予告なく停止するリスク**をアプリ内ヘルプとREADMEに明記。
- Ingest URLはMVPから可変だが、デフォはTopaz。
- ライセンス: 本ソフトは**MIT**で公開。同梱GStreamerはLGPLのためGPL汚染なし (ezTopazのlibx264/GPL問題が解消)。
- テレメトリなし、自動更新なし。手動DLでの更新。
- GStreamer MSVCランタイムの取得元はCIでpinする (design §13.2)。

---

## 10. 受入基準

| # | シナリオ | 期待結果 | MVP |
|---|---|---|---|
| AC-01 | 未設定で配信開始押下 | 各項目でエラー、配信開始されない | ● |
| AC-02 | 全画面/ウィンドウ切替→プレビュー切替 | 1秒以内に更新 | ● |
| AC-03 | マイクミュート→VU 0、配信に無音(システム音は載る) | ミキシング分離 | ● |
| AC-04 | Spotifyのみ「含める」→他アプリ音が載らない | WASAPI分離確認 | ● |
| AC-04c | Chrome+Spotifyを複数選択→2アプリの音がミックスされ他は載らない | 複数アプリミックス確認 | ● |
| AC-04d | マイクOFF→マイク音が載らずアプリ/システム音のみ | マイクオンオフ確認 | ● |
| AC-05 | 「高」で配信→ ffprobeで2000k±10%, 320k, 60fps, yuv420p, GOP 2秒 | 上限遵守 | ● |
| AC-05b | 1080p選択→警告表示、配信は2000kで実行 | 警告付プロファイル | ● |
| AC-06 | Key `test-key-123` でコピー→ `rtspt://topaz.chat/live/test-key-123` | Questも同様 | ● |
| AC-06b | Ingest URLを変更→そちらへ配信 | 可変Ingest | ● |
| AC-07 | ネット切断→3回リトライ、復帰後継続 | | △ |
| AC-08 | Win10 2004+/11で起動・配信・視聴(Topaz Player)成功 | 実機確認 | ● |
| AC-09 | 言語切替 ja/en→UIが即時切替、再起動後も保持 | 日英対応 | ● |
| AC-10 | エンコーダ手動でx264選択→ x264で配信、自動はHW優先 | 手動選択 | ● |
| AC-11 | GStreamer未導入の素のWindowsでNSIS導入→同梱ランタイムで配信可 | 同梱完結 | ● |
| AC-12 | Linux (Flatpak) で起動→Portal選択→配信→ffprobe確認 | Wayland+PipeWire実機 | ● |

> MVP: ●=MVPリリース判定に使用 / △=F-ST-04がShouldのためMVP可否未決

---

## 11. リスク

| リスク | 影響 | 対策 |
|---|---|---|
| TopazChat仕様変更/停止 | 配信不可 | Ingest可変で代替へ切替可能に |
| GStreamer同梱肥大 | 配布サイズ増 | プラグインallowlist同梱 (design §13.2)。不足時は手動選択でx264へ |
| HWエンコーダなし | 高負荷 | x264フォールバック + 低画質自動提案 |
| MSVCランタイムのDLL地獄 | 起動失敗 | `ensure_bundled_runtime` で同梱優先+フォールバック、起動時probeで不在を検出して赤表示 |
| FlatpakのPipeWire権限不足 | Linuxで音声が取れない | manifestに `--filesystem=xdg-run/pipewire-0` を指定。実機E2Eで確認 |
| `zerolatency`系の灰色画面 | 視聴不可 | 全エンコーダでB-frames 0/GOP固定/CBRを明示し、tune系は使わない。NVENCは `hp` (High Performance) |

---

## 付録 A. 参考リンク

- TopazChat GitHub: https://github.com/TopazChat/TopazChat
- TopazChat Player (BOOTH): https://booth.pm/ja/items/1752066
- TopazChat Fanbox (支援): https://tyounanmoti.fanbox.cc/
- GStreamer: https://gstreamer.freedesktop.org/ / Rust bindings: https://gitlab.freedesktop.org/gstreamer/gstreamer-rs

## 付録 B. 変更履歴

| 版 | 日付 | 変更 |
|---|---|---|
| 0.1 | 2026-09-04 | 初版作成 (ezTopaz v0.3.2からfork: Windows専用化、FFmpeg→GStreamer、Linux/X11/Wayland/Portal/PipeWire/ffmpeg-sidecar/named-pipe/ddagrab関連を削除、AC-11同梱完結を追加) |
| 0.2 | 2026-09-10 | Linux/Flatpak (Portal ScreenCast + PipeWire) をスコープへ追加。レビュー修正を反映: NVENC `high-performance`、設定永続化 (F-CF-02/05)、ファイルログ (F-CF-04)、A/V同期とappsrc backpressure、マイクデバイス選択 (F-AU-03/05)、per-app 音量/ミュートUI (F-AU-04)、カーソルON/OFF (F-SC-04)、プロファイル export/import (F-CF-03)、AC-12 |
| 0.3 | 2026-09-11 | GUI を Tauri + React から Rust + egui/eframe へ移行 (§4.2)。UI要件に「直線的でつながりのあるデザイン」「絵文字・グラデーション禁止」を追加 (§7)。設定画面を全画面ペイン化、i18n を同梱JSON化 |
