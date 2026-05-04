<!-- markdownlint-disable MD033 MD041 -->

<br/>

<p align="center">
  <!-- Icon in design — replace src once asset is ready. -->
  <img src="https://github.com/shinkuan/RandomStuff/assets/35415788/db94b436-c3d4-4c57-893e-8db2074d2d22" width="50%">
</p>

<h1 align="center">Akagi</h1>

<p align="center">
  <i>「死ねば助かるのに………」 - 赤木しげる</i>
  <br/><br/>
  針對 <b>雀魂</b>、<b>天鳳</b> 以及更多平台的即時麻將 AI 輔助工具。<br/>
  Akagi V3
  <br/><br/>
  <a href="https://discord.gg/Z2wjXUK8bN">在 Discord 上提問</a>
  ·
  <a href="https://github.com/shinkuan/Akagi/issues">回報 Bug</a>
  ·
  <a href="https://github.com/shinkuan/Akagi/issues">功能建議</a>
  ·
  <a href="https://deepwiki.com/shinkuan/Akagi">DeepWiki</a>
</p>

<p align="center">
  <a href="https://github.com/shinkuan/Akagi/stargazers"><img src="https://img.shields.io/github/stars/shinkuan/Akagi?logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/shinkuan/Akagi/releases"><img src="https://img.shields.io/github/v/release/shinkuan/Akagi?label=release&logo=github&include_prereleases" alt="Latest release" /></a>
  <a href="https://github.com/shinkuan/Akagi/issues"><img src="https://img.shields.io/github/issues/shinkuan/Akagi?logo=github" alt="Open issues" /></a>
  <a href="./LICENSE.txt"><img src="https://img.shields.io/badge/license-Apache%202.0-blue?logo=apache" alt="License: Apache-2.0" /></a>
  <a href="https://github.com/shinkuan/Akagi/actions/workflows/release.yml"><img src="https://img.shields.io/github/actions/workflow/status/shinkuan/Akagi/release.yml?branch=v3&logo=githubactions&label=build" alt="Build status" /></a>
  <a href="https://discord.gg/Z2wjXUK8bN"><img src="https://img.shields.io/discord/1192792431364673577?label=discord&logo=discord&color=7289DA" alt="Discord" /></a>
  <a href="https://deepwiki.com/shinkuan/Akagi"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki" /></a>
</p>

<p align="center">
  其他分支：
</p>

<p align="center">
  <a href="https://github.com/shinkuan/Akagi/tree/v2"><img src="https://img.shields.io/badge/Akagi-v2_(Python)-blue?logo=github" alt="v2 branch" /></a>
  <a href="https://github.com/shinkuan/Akagi/tree/ng"><img src="https://img.shields.io/badge/Akagi-NG_(Electron)-blue?logo=github" alt="NG branch" /></a>
</p>

<p align="center">
  <a href="./README.md">English</a>
  ·
  <b>繁體中文</b>
  ·
  <a href="./README.zh-CN.md">简体中文</a>
</p>

---

以下為AI機翻

## 簡介

> 本專案的目的，是讓你能即時掌握自己在麻將對戰中的表現並從中學習。
> 本專案僅供 **教育用途**。作者不對使用者的任何行為負責。
> 遊戲開發商與發行商保留對違反其服務條款者採取行動的權利；
> 任何後果（如帳號停權等）皆由使用者自行承擔。

Akagi 透過本機 Proxy 或內建瀏覽器監看你在雀魂 / 天鳳的對局，
鏡射遊戲狀態，並在可拖曳的 HUD 中顯示 **向聽**、**待牌**、
**和牌率**、**聽牌率**、**對各家放銃風險**，以及
**推薦切牌**。若放入符合 mjai 協定的 bot（例如 Mortal），
HUD 在每巡也會顯示該 bot 的建議。

## 截圖

<img width="2559" height="1439" alt="image" src="https://github.com/user-attachments/assets/da9e7cce-d8ef-4e6e-807b-f6f54013cf22" />

https://github.com/user-attachments/assets/42812e85-ccf0-49fd-b825-adbb5b7b58b0

https://github.com/user-attachments/assets/2ce7cb71-8b25-4895-a12b-0a638665dcab

---

## 目錄

**使用者**
- [功能](#功能)
- [支援的平台](#支援的平台)
- [快速開始](#快速開始)
- [設定檔](#設定檔)
- [Bots](#bots)
- [對局歷史](#對局歷史)
- [紀錄與診斷](#紀錄與診斷)
- [疑難排解](#疑難排解)
- [Roadmap](#roadmap)

**開發者**
- [架構](#架構)
- [技術堆疊](#技術堆疊)
- [專案結構](#專案結構)
- [從原始碼建置](#從原始碼建置)
- [測試](#測試)
- [Releases 與 CI](#releases-與-ci)
- [參考資料](#參考資料)
- [授權與致謝](#授權與致謝)
- [鳴謝](#鳴謝)

---

## 功能

- **即時 HUD** — 向聽、待牌、和牌率、聽牌率、對各家放銃
  風險、推薦的進攻 / 防守切牌。可拖曳、可縮放的牌格佈局
  會持久化於 local storage。
- **兩種抓包模式**
  - **MITM proxy**（預設） — 系統層級；需一次性的 CA 信任。
  - **Chromium** — 由 Akagi 啟動受控的 Chromium 系列瀏覽器，
    透過 Chrome DevTools Protocol 攔截 WebSocket 訊框。
    無需設定 proxy 或安裝憑證；直接在啟動的視窗中遊玩即可。
- **可插拔的 mjai bot** — 在設定一鍵安裝 Mortal，或將
  任意 `bot.py` 放入 `mjai_bot/<name>/`。可依模式切換：
  `bot.active_4p` 與 `bot.active_3p` 會依牌桌人數自動套用。
- **對局歷史** — 每場結束的對局會自動記錄。歷史頁籤顯示
  名次圓餅圖、可選計分規則的累積 PT 折線圖（雀魂段位 /
  天鳳段位 / 自訂 uma），以及細部統計（和牌率、放銃率、
  立直率、副露率、流局率、平均和牌 / 放銃點數、平均和牌
  巡目、役滿 / 流局滿貫次數）。
- **紀錄檢視** — **Diagnostic** 頁籤即時 tail 應用程式紀錄，
  可依模組過濾；**Inspector** 頁籤顯示原始 WebSocket 訊框
  → mjai 事件 → bot 反應，附訊框數量與 meta 檢視。
- **首次啟動設定** — 語言 → 平台 → 抓包模式 →
  CA 信任 / Chromium 選擇 → bot 安裝 → 完成。
- **多語系** — English、日本語、繁體中文、简体中文。
  可在設定精靈或側邊欄即時切換，覆蓋整個 UI。
- **三麻** — 完整流程：bridge、tracker、snapshot、
  分析、依模式 bot 路由、歷史統計、3p uma 表。

## 支援的平台

| 平台 | 四麻 | 三麻 | AutoPlay |
|---|:---:|:---:|:---:|
| **雀魂（Mahjong Soul / Majsoul）** | &check; | &check; | （規劃中） |
| **天鳳（Tenhou）** | &check; | &check; | &cross; |
| **Riichi City** | （規劃中） | （規劃中） | &cross; |
| **Amatsuki** | （規劃中） | （規劃中） | &cross; |

---

## 快速開始

### A. 安裝官方 Release

Akagi 以 portable zip 發佈 — 每個平台一個自帶所需檔案的資料夾。
從 [Releases](https://github.com/shinkuan/Akagi/releases) 下載
對應作業系統的 zip,解壓縮到任何你有寫入權限的位置(例如
`~/Apps/`、桌面),然後直接執行裡面的 binary 即可。設定檔、紀錄、
歷史對局、CA 憑證以及 bot 都會建立在 binary 旁邊,所以搬移 /
備份 / 解除安裝就只是搬移 / 複製 / 刪除整個資料夾。

| OS | 檔案 | 備註 |
|---|---|---|
| Windows | `akagi-<version>-windows-x64.zip` | x86_64。需要 WebView2(Win10 1803+ 與 Win11 已預載)。SmartScreen 會警告 — 點 *More info → Run anyway*。 |
| macOS | `akagi-<version>-macos-arm64.zip` | Apple Silicon。未簽章,解壓後執行一次 `xattr -cr <解壓後資料夾>`,或第一次右鍵 → *Open*。 |
| Linux | `akagi-<version>-linux-x64.zip` | 在 `ubuntu-22.04` 上建置(glibc 2.35+)。需要 WebKit2GTK 4.1(`apt install libwebkit2gtk-4.1-0` / `dnf install webkit2gtk4.1` / `pacman -S webkit2gtk-4.1`)。 |

每個 zip 都將 `python-build-standalone` 3.12 + `uv` 一併放在
binary 旁邊,bot 開箱即用,不需另外安裝系統 Python。

首次啟動時會引導你完成語言、平台、抓包
模式、選用的 bot 安裝(Mortal)以及 CA 信任(僅 MITM
模式才需要)。

### B. Chromium 模式（不需信任 CA）

最簡單的方式。完成設定後：

1. 設定 → **Capture** → 將 Mode 設為 **Chromium**。
2. 點擊 **Detect** 自動尋找 Chrome / Edge / Brave / Chromium，
   或手動設定 `capture.chromium.executable`。
3. Akagi 會以獨立的個人資料啟動瀏覽器，目錄位於
   `<config_root>/chrome-profile`。登入雀魂後即可開始遊玩。

透過 Chrome DevTools Protocol 攔截 — 不需系統 proxy、
不需憑證。

### C. MITM 模式

系統層級的 proxy，搭配位於 `./ca/` 的自簽根 CA：

1. 在作業系統 / 瀏覽器的憑證儲存區信任
   `./ca/akagi-ca.crt`（或 `.cer` / `.pem` / `.der`）。
2. 將遊戲客戶端的流量導向 `127.0.0.1:23410`。
   健康檢查：`GET /ping` → `pong`。
3. Windows 上常用 [Proxifier](https://www.proxifier.com/)
   把指定應用程式導向 proxy。

---

## 設定檔

設定檔 `config.toml` 位於可執行檔旁（或你以 `--config` 指
向的位置）。透過設定 UI 儲存的修改會熱重載對應子系統 —
capture / proxy / bot active 槽位無需重啟整個應用即可生效。

```toml
[general]
language = "en"

[logging]
dir       = "./logs"
level     = "info"
all_level = "warn"

[platform]
kind = "Majsoul"

[proxy]
enabled = true
addr    = "127.0.0.1:23410"
ca_dir  = "./ca"

[capture]
mode = "mitm"               # 或 "chromium"

[capture.chromium]
executable    = ""          # 留空 = 自動偵測
user_data_dir = ""          # 留空 = <config_root>/chrome-profile
start_url     = "https://game.maj-soul.com/1/"
cft_channel   = "stable"
force_cft     = false
extra_args    = []

[bot]
enabled   = true
active_4p = "mortal"        # 用於四麻
active_3p = "mortal3p"      # 用於三麻；留空 = 不啟用
auto_sync = true
dir       = "./mjai_bot"
```

<details>
<summary>設定檔位置（解析順序）</summary>

1. `--config <path>` CLI 旗標。
2. `<exe_dir>/configs/config.toml`。
3. 當前工作目錄下的 `./configs.toml`。
4. 以上皆不存在時，首次啟動會將預設值寫入
   `<exe_dir>/configs/config.toml`。

舊版設定（仍使用單一 `active = "..."` 鍵）載入時會自動
遷移為 `active_4p`。
</details>

---

## Bots

### 安裝 Bot

在 **Bots** 頁籤可直接從 GitHub release 安裝 bot：

範例:

- Repo：`shinkuan/Akagi-MjaiBot-Mortal`
- 四麻：`release4p.zip`
- 三麻：`release3p.zip`

IPC 指令 `install_bot_from_github(repo, asset_glob?, name?)`
會抓取最新 release zip，解壓至 `mjai_bot/<name>/`，驗證
`bot.py`，並執行一次 `uv sync`。後續啟動很快 — sync
會根據 `mjai_bot/<name>/.akagi/synced.stamp` 戳記決定是否
跳過。

### 依模式切換的 bot

`bot.active_4p` 與 `bot.active_3p` 互相獨立。Akagi 會在開
局時依牌桌人數選用對應的 bot。將某個槽位留空即可在該
模式下僅使用 **分析功能**（不顯示 bot 建議）。

### 自行撰寫 bot

```
mjai_bot/<name>/
├── bot.py            # JSONL stdin → JSONL stdout
├── pyproject.toml    # requires-python = ">=3.12"
├── manifest.toml     # 選填 — supported_modes、設定 schema
└── README.md
```

`bot.py` 從 stdin 每行讀取一個 mjai 事件 JSON 陣列，並從
stdout 每行寫出一個 mjai 動作物件（無動作時輸出
`{"type":"none"}`）。Akagi 會把 stderr 內容寫入應用程式
紀錄中的 `bot=<name>` 條目。

完整協定、manifest schema 以及 secret 欄位處理請見
[`src/bot/README.md`](./src/bot/README.md)。
[`mjai_bot/example/`](./mjai_bot/example/) 為一個 in-tree、
可運作的規則型範例 bot。

### AGPL 邊界

Bot 以 Akagi 啟動的 **獨立 OS 子行程** 執行。通訊嚴格透
過 stdin / stdout 上的 JSONL 進行 — 沒有 in-process 連結、
沒有共享位址空間、沒有 FFI。這是刻意設計的授權邊界：
AGPL 授權的 bot（例如連結 libriichi 的 Mortal）會留在
其自己的行程內，因此把它放入 `mjai_bot/<name>/` **不會**
讓 Akagi 成為該 bot 的衍生作品。

---

## 對局歷史

每一場乾淨結束的對局（產生了 `end_game` mjai 事件）都會
被持久化到 `<config_root>/history/`：

```
<config_root>/history/
├── index.jsonl              # 每行一筆 GameRecord（以 ULID 為 key）
└── games/
    └── <ulid>.mjai.jsonl    # 完整事件流的副本
```

中途斷線會在 buffer 中留下未完成的紀錄並被靜默丟棄 —
只有完整對局會落到磁碟。

前端 **History** 頁籤顯示：

- **名次圓餅圖** — 1/2/3/4 名分布（三麻只有 3 片）。
- **累積 PT 折線圖** — 可選擇計分規則：
  - **雀魂**：選擇 `場次`（銅 / 銀 / 金 / 玉 / 王座）與
    `段位`（初心 1 星 → 魂天）。
  - **天鳳**：選擇 `段位`（新人 → 天鳳位，共 21 階）。
  - **自訂**：直接編輯 uma 與段位獎金陣列。
  切換規則 / 段位會立即重繪 — 不需要 backend round-trip。
- **細部統計** — 和牌率、放銃率、立直率、副露率、
  流局率、平均和牌 / 放銃點數、平均和牌巡目、
  役滿 / 流局滿貫次數。
- **對局清單** — 可依平台 / 人數 / 東風或半莊 / 日期過濾。
  點選列即可看到最終排名與該局統計；垃圾桶圖示會同時
  刪除 index 條目與該局的 `.mjai.jsonl`。

PT 規則與過濾條件會持久化於 `localStorage`。Bridge 啟動
時從 backend 載入紀錄，並透過 `history-recorded` Tauri
事件保持同步。

數學細節、儲存 schema，以及如何新增平台 / 統計欄位 /
過濾維度請見 [`src/history/README.md`](./src/history/README.md)。

---

## 紀錄與診斷

每次 session 的紀錄會落在 `<log_dir>/<YYYYMMDD-HHMMSS>/`：

```
<log_dir>/<session>/
├── all.log                       # 所有 tracing 輸出彙整
├── <target>.log                  # 依模組過濾的紀錄
├── proxy.binlog                  # 原始 WS 二進位訊框
├── majsoul/<flow_id>.log         # 每條 WebSocket flow 的 JSON 紀錄
├── majsoul/<flow_id>.mjai.jsonl  # 每場對局的 mjai 事件流
└── inspector.jsonl               # Inspector 看到的訊框
```

前端 **Logs** 路由有兩個頁籤：

### Diagnostic

可過濾的應用程式紀錄。可依等級（trace / debug / info /
warn / error）與模組過濾。可即時 tail 或瀏覽過去的
session；點選列可看到原始結構化欄位與來源位置。
**Open Folder** 按鈕會在系統檔案管理員中開啟該 session
資料夾。

### Inspector

協定層級的訊框檢視器。共三類條目：

- **WS Frame** — 原始二進位（base64 截短）加上 bridge
  的初步解析結果。
- **MjaiEvent** — 流向 bot 的解碼後事件。
- **BotReaction** — bot 的回應，含 `meta` 欄位
  （信心度 / q-values / bot 想送出的任何資訊）。

訊框計數會顯示每個 WS 訊框產生了多少個 mjai 事件，
在排查 bot 或 bridge 問題時很有用。

---

## 疑難排解

> [!TIP]
> 重現問題後，存下 `<log_dir>/<session>/` 整個 session
> 資料夾 — 內含應用紀錄、原始訊框、mjai 事件、bot meta，
> 是回報有用 bug 報告所需的所有資訊。

- **MITM 模式抓不到封包。** 確認 `./ca/akagi-ca.crt`
  已在系統憑證庫中信任。確認 proxy 已啟動：
  `curl http://127.0.0.1:23410/ping` 應回應 `pong`。
  確認你的 proxy 重導工具（Proxifier / 系統 proxy）
  正把遊戲客戶端送到正確的 host:port。
- **Chromium 模式抓不到封包。** Detect 沒找到瀏覽器。
  在設定或 `config.toml` 裡手動設定
  `capture.chromium.executable`。如果瀏覽器有啟動但沒
  訊框流入，檢查 `--remote-debugging-port` 是否被其他
  擴充功能擋下。
- **Bot 卡在 `Loading{SyncingDeps}`。** 首次 `uv sync`
  會慢 — 在 Diagnostic 頁籤觀察 `bot=<name>` 的訊息。
  若一直未完成，刪除
  `mjai_bot/<name>/.akagi/synced.stamp` 後重試。
- **Bot 對局途中崩潰。** Inspector 頁籤可顯示 bot 死前
  看到的最後一個訊框；附在 bug 報告裡。
- **三麻挑了錯的 bot。** 檢查設定 → Bot 中的
  `bot.active_3p` — 它與 `bot.active_4p` 互相獨立。
- **要去哪求助？** 聊天請至
  [Discord](https://discord.gg/Z2wjXUK8bN)，
  追蹤型的 bug 與功能建議請至
  [GitHub Issues](https://github.com/shinkuan/Akagi/issues)。

---

## Roadmap

alpha.8 已完成：

- [x] 三麻 — 完整流程
- [x] 天鳳 bridge（僅觀戰）
- [x] 對局歷史持久化 + History 頁籤（名次圓餅 / PT 圖 / 統計）
- [x] 紀錄檢視（Diagnostic + Inspector）
- [x] i18n：en / ja / zh-TW / zh-CN，含設定精靈語言選擇
- [x] 從 GitHub release 安裝 bot
- [x] Chromium 抓包模式（不需信任 CA）

規劃中：

- [ ] **Riichi City** 平台支援
- [ ] **Amatsuki** 平台支援
- [ ] **自訂主題**（前端 theming hook）
- [ ] **AutoPlay**（先支援雀魂；由 bot 自主控制牌桌，
      類似原版 Akagi 在 Windows 的 AutoPlay）
- [ ] **前端打磨** — 牌型佈局、動畫、無障礙
- [ ] **天鳳 autoplay**（目前僅觀戰）

詳細的 bug 追蹤請至
[GitHub Issues](https://github.com/shinkuan/Akagi/issues)。

---
---

## 架構

單一 Rust 執行檔。各子系統只持有自己的 bus handle，
彼此互不擁有。
[`src/event_bus.rs`](./src/event_bus.rs) 是所有 channel
類型的單一真相來源。

```
                ┌────────────────────────┐
   遊戲客戶端 ─│  capture (mitm | cdp)  │── CA 位於 ./ca（僅 mitm）
   WebSocket   └─────────┬──────────────┘
                          ▼
                ┌────────────────────────┐
                │  bridge::<platform>    │   wire bytes → MjaiEvent
                └─────────┬──────────────┘
                          ▼ MjaiBus
       ┌──────────────────┼──────────────────┐
       ▼                  ▼                  ▼
  game_state::tracker   bot::manager     ipc forwarder
       │                  │                  │
       ▼ PostBus          ▼ BotResponseBus   ▼ app.emit
  analysis::runner   subprocess (uv)    Tauri webview
       │
       ▼ AnalysisBus
       └──► ipc forwarder ──► app.emit
```

[`src/lib.rs`](./src/lib.rs) 在啟動時把這些 bus 接起來。
前端透過六個 push 事件（`mjai-event`、`bot-response`、
`bot-status`、`proxy-status`、`notify`、`history-recorded`）
與 backend 溝通，pull 指令的清單請見
[`src/ipc/README.md`](./src/ipc/README.md)。

## 技術堆疊

| 層級 | 技術 |
|---|---|
| Shell | [Tauri](https://tauri.app) 2 |
| Backend | Rust（edition 2021）、`tokio`、`tracing`、`clap` |
| MITM | [`hudsucker`](https://crates.io/crates/hudsucker) 0.24（`rcgen-ca`、`rustls-client`） |
| CDP capture | [`chromiumoxide`](https://crates.io/crates/chromiumoxide) 0.9 |
| 麻將引擎 | [`riichienv-core`](https://github.com/smly/RiichiEnv) 0.4 |
| Protobuf | `prost` 0.14 + `prost-reflect` 0.16 |
| 前端 | [React](https://react.dev) 19、TypeScript、[Vite](https://vitejs.dev) 8 |
| 樣式 | [Tailwind CSS](https://tailwindcss.com) v4、[shadcn/ui](https://ui.shadcn.com)（Radix Nova preset） |
| 狀態 | [Zustand](https://github.com/pmndrs/zustand) |
| 圖表 | [Recharts](https://recharts.org) |
| 牌型渲染 | [`<mah-gen>`](https://github.com/eric200203/mahgen) Web Component |
| i18n | [react-i18next](https://react.i18next.com) |
| Bot 執行環境 | `python-build-standalone` 3.12 + [`uv`](https://github.com/astral-sh/uv)（依平台打包） |

## 專案結構

```
.
├── src/
│   ├── analysis/      向聽 / 待牌 / 和牌率 / 風險 / 切牌搜尋
│   ├── bot/           Registry、Python runtime、JSONL 子行程執行器
│   ├── bridge/        各平台協定 → MjaiEvent
│   │   ├── majsoul/   雀魂（liqi protobuf）
│   │   └── tenhou/    天鳳（JSON tag stream，僅觀戰）
│   ├── capture/       抓包 backend 抽象（mitm | chromium）
│   ├── config/        AppConfig（TOML）區段與解析
│   ├── event_bus.rs   子系統間的 broadcast channel
│   ├── game_state/    riichienv 驅動的鏡射、snapshot、mahgen view
│   ├── history/       對局回放儲存與索引
│   ├── inspector/     訊框 / 事件 / bot reaction broadcaster
│   ├── ipc/           Tauri 指令、app state、capture supervisor
│   ├── logger/        每 session 紀錄目錄與每 target 檔案 appender
│   ├── proxy/         透過 hudsucker 的 MITM HTTP/HTTPS/WS；CA 位於 ./ca
│   ├── schema/        MjaiEvent enum 與 IPC payload 類型
│   └── lib.rs         啟動與接線
├── mjai_bot/
│   └── example/       in-tree 規則型向聽優化器
├── frontend/          React + Vite + Tailwind + shadcn UI
│   └── src/
│       ├── routes/    Overview / GameDashboard / Bots / History / Logs / Settings / Setup / InspectorView / DiagnosticView
│       ├── tiles/     儀表板磚塊（header、hands、opponents、analysis…）
│       ├── stores/    Zustand slice（game、analysis、bot、proxy、notify、layout、config）
│       └── i18n/      en / ja / zh-TW / zh-CN
├── tests/             整合測試
├── capabilities/      Tauri 權限
├── icons/             應用程式圖示
├── tauri.conf.json    視窗與 bundle 設定
└── Cargo.toml
```

各模組的開發者指南位於對應的 `src/*/README.md`。

## 從原始碼建置

**前置需求**

- Rust（最新 stable，1.80+）
- Node.js 20+ 與 npm
- Tauri 2 系統相依：
  - **Linux**：`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、
    `libayatana-appindicator3-dev`、`librsvg2-dev`、
    `protobuf-compiler`
  - **macOS**：Xcode Command Line Tools
  - **Windows**：WebView2（Windows 11 已預先安裝）

**執行 / 建置**

```bash
# Debug — 啟動 GUI;Vite dev-server 由 Tauri 代理
cargo run

# 指定設定檔路徑
cargo run -- --config ./my-config.toml

# 為當前目標建置 portable zip
cargo install tauri-cli --locked          # 若尚未安裝
bash scripts/fetch-runtime.sh             # 抓取 runtime/<triple>/
cargo tauri build --no-bundle             # 產出 target/<triple>/release/akagi
bash scripts/package-zip.sh <target-triple>
# → dist/akagi-<version>-<os>-<arch>.zip

# 僅啟動前端 dev(Vite 在 :1420)
cd frontend && npm ci && npm run dev
```

**內建執行環境**

`scripts/fetch-runtime.sh <target-triple>` 會下載對應目標的
`python-build-standalone` 3.12 與 `uv`,並放置於 `runtime/`。
`scripts/package-zip.sh` 接著會把這個目錄複製到 zip 內的
binary 旁邊;`src/bot/runtime.rs` 在執行時會用 exe-adjacent
方式找到它,因此最終的 App 即使使用者沒有系統 Python 也能運作。

## 測試

整合測試位於 [`tests/`](./tests/)：

| 檔案 | 涵蓋範圍 |
|---|---|
| `analysis_pipeline.rs` | 端到端分析（事件 → 向聽 → 切牌建議） |
| `analysis_bench.rs` | hot path 效能 |
| `bot_lifecycle.rs` | 安裝 → sync → spawn → 來回通訊 |
| `example_bot.rs` | 規則型參考 bot 跑合成對局 |
| `mortal_zip_layout.rs` | 驗證 Mortal release zip 結構 |

```bash
cargo test               # 所有測試（含整合測試）
cargo test --release     # 用於效能 bench
```

## Releases 與 CI

GitHub Actions [`release.yml`](./.github/workflows/release.yml)
會在 tag 推送(`v3.*`)或手動觸發時建置,每個目標產出一個
portable zip:

| OS runner | 目標 | 產出檔案 |
|---|---|---|
| `ubuntu-22.04`(glibc 2.35) | `x86_64-unknown-linux-gnu` | `akagi-<version>-linux-x64.zip` |
| `macos-14` | `aarch64-apple-darwin` | `akagi-<version>-macos-arm64.zip` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `akagi-<version>-windows-x64.zip` |

每個 zip 都將 `python-build-standalone` 3.12 + `uv` 一併放在
binary 旁邊,bot 不需另外安裝系統 Python 即可運作。

Tag 必須位於 `v3` 分支。

## 參考資料

| 來源 | 應用於 | 用途 |
|---|---|---|
| [mjai JSONL 規格（Gimite）](https://gimite.net/pukiwiki/index.php?Mjai%20%E9%BA%BB%E9%9B%80AI%E5%AF%BE%E6%88%A6%E3%82%B5%E3%83%BC%E3%83%90) | `src/schema/mjai/` | `MjaiEvent` enum 與 bot wire 協議 — 15 種事件、tile-string 格式、狀態機規則。 |
| [`EndlessCheng/mahjong-helper`](https://github.com/EndlessCheng/mahjong-helper)（Go 分析 CLI） | `src/analysis/` | `util/` 的直接 Rust 移植 — 向聽、待牌、和牌率、聽牌率、風險模型、切牌搜尋。 |
| [`Xerxes-2/MajsoulMax-rs`](https://github.com/Xerxes-2/MajsoulMax-rs)（Rust MITM proxy，**GPL-3.0**） | `src/proxy/handler.rs`、`src/bridge/majsoul/parser.rs`、`src/bridge/majsoul/proto/liqi.proto` | 雀魂 5 層 WS wire 格式參考（type byte → Wrapper → 內層訊息 → action protobuf）。**僅參考格式 — 未複製程式碼。** |
| [`smly/RiichiEnv`](https://github.com/smly/RiichiEnv)（Rust RL env + Python bindings） | `Cargo.toml`（`riichienv-core` 相依）、`src/analysis/`、`src/game_state/` | 牌 / 手牌 / 向聽 / 役 / 計分原語 + 遊戲狀態模型。分析引擎與 game tracker 都建構在它之上。 |
| [`eric200203/mahgen`](https://github.com/eric200203/mahgen)（麻將牌渲染 DSL） | `src/game_state/mahgen_view.rs`、前端 `<mah-gen>` | DSL 語法，用於後端預先編碼手牌 / 副露 / 河字串。 |
| [`smly/mjai.app`](https://github.com/smly/mjai.app)（麻將 AI 競賽平台） | `mjai_bot/`、`src/bot/` | bot 子行程慣例 — JSONL stdin/stdout、argv `python bot.py <player_id>`、`AKAGI_PLAYER_ID` 環境變數、批次結尾 flush 點。 |
| [`shinkuan/Akagi`](https://github.com/shinkuan/Akagi)（原版 Akagi，Python） | 架構 / 行為對齊 | 我們所重現的原始功能集：MITM proxy、mjai bridge、可插拔 bot、推薦 HUD。 |

## 授權與致謝

Akagi v3 採用 [Apache License 2.0](./LICENSE.txt)。
Copyright 2026 Shinkuan。第三方致謝資訊位於
[`NOTICE`](./NOTICE) — 請與授權一同閱讀。依 Apache-2.0
§4(d)，重新散布時必須附上這兩個檔案。

**內附 / 連結原始碼**

- **mahjong-helper**（MIT） — `src/analysis/` 為 `util/` 的 Rust 移植。
- **riichienv-core** / RiichiEnv（Apache-2.0） — Cargo 相依。
- **mahgen**（MIT） — DSL + `<mah-gen>` custom element。

**僅供參考**（未複製程式碼；列於 `NOTICE` 以示致謝）

- **MajsoulMax-rs**（GPL-3.0） — 僅參考雀魂 WS wire 格式。
- **mjai 規格**（Gimite） — bot wire 協議。
- **mjai.app** — bot 子行程慣例。

## 鳴謝

- [Akagi](https://github.com/shinkuan/Akagi)（Python，v2）與
  [AkagiNG](https://github.com/shinkuan/AkagiNG)（Electron + Python） —
  v3 所立基的前作。
- [`mjai.app`](https://github.com/smly/mjai.app) 以及 Gimite
  制定的 mjai 規格 — 讓可插拔 bot 成為可能的協議。
- [Discord](https://discord.gg/Z2wjXUK8bN) 社群提供的 bug
  回報、模型貢獻與意見回饋。
