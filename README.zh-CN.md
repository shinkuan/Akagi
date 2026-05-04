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
  面向 <b>雀魂</b>、<b>天凤</b> 以及更多平台的实时麻将 AI 辅助工具。<br/>
  Akagi V3
  <br/><br/>
  <a href="https://discord.gg/Z2wjXUK8bN">在 Discord 上提问</a>
  ·
  <a href="https://github.com/shinkuan/Akagi/issues">报告 Bug</a>
  ·
  <a href="https://github.com/shinkuan/Akagi/issues">功能建议</a>
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
  <a href="./README.zh-TW.md">繁體中文</a>
  ·
  <b>简体中文</b>
</p>

---

以下为AI机翻

## 简介

> 本项目的目的，是让你能实时掌握自己在麻将对局中的表现并从中学习。
> 本项目仅供 **教育用途**。作者不对使用者的任何行为负责。
> 游戏开发商与发行商保留对违反其服务条款者采取行动的权利；
> 任何后果（如账号封禁等）皆由使用者自行承担。

Akagi 通过本机 Proxy 或内置浏览器监听你在雀魂 / 天凤的对局，
镜像游戏状态，并在可拖拽的 HUD 中显示 **向听**、**听牌**、
**和牌率**、**听牌率**、**对各家放铳风险**，以及
**推荐切牌**。若加入符合 mjai 协议的 bot（例如 Mortal），
HUD 在每巡也会显示该 bot 的建议。

## 截图

<img width="2559" height="1439" alt="image" src="https://github.com/user-attachments/assets/da9e7cce-d8ef-4e6e-807b-f6f54013cf22" />

https://github.com/user-attachments/assets/42812e85-ccf0-49fd-b825-adbb5b7b58b0

https://github.com/user-attachments/assets/2ce7cb71-8b25-4895-a12b-0a638665dcab

---

## 目录

**用户**
- [功能](#功能)
- [支持的平台](#支持的平台)
- [快速开始](#快速开始)
- [配置文件](#配置文件)
- [Bots](#bots)
- [对局历史](#对局历史)
- [日志与诊断](#日志与诊断)
- [疑难排查](#疑难排查)
- [Roadmap](#roadmap)

**开发者**
- [架构](#架构)
- [技术栈](#技术栈)
- [项目结构](#项目结构)
- [从源码构建](#从源码构建)
- [测试](#测试)
- [Releases 与 CI](#releases-与-ci)
- [参考资料](#参考资料)
- [许可与致谢](#许可与致谢)
- [鸣谢](#鸣谢)

---

## 功能

- **实时 HUD** — 向听、听牌、和牌率、听牌率、对各家放铳
  风险、推荐的进攻 / 防守切牌。可拖拽、可缩放的牌格布局
  会持久化到 local storage。
- **两种抓包模式**
  - **MITM proxy**（默认） — 系统级；需一次性的 CA 信任。
  - **Chromium** — 由 Akagi 启动受控的 Chromium 系列浏览器，
    通过 Chrome DevTools Protocol 拦截 WebSocket 帧。
    无需配置 proxy 或安装证书；直接在启动的窗口中游玩即可。
- **可插拔的 mjai bot** — 在设置一键安装 Mortal，或将
  任意 `bot.py` 放入 `mjai_bot/<name>/`。可按模式切换：
  `bot.active_4p` 与 `bot.active_3p` 会按牌桌人数自动启用。
- **对局历史** — 每场结束的对局会自动记录。历史标签页显示
  名次饼图、可选计分规则的累计 PT 折线图（雀魂段位 /
  天凤段位 / 自定义 uma），以及详细统计（和牌率、放铳率、
  立直率、副露率、流局率、平均和牌 / 放铳点数、平均和牌
  巡目、役满 / 流局满贯次数）。
- **日志查看** — **Diagnostic** 标签页实时 tail 应用日志，
  可按模块过滤；**Inspector** 标签页显示原始 WebSocket 帧
  → mjai 事件 → bot 反应，附帧数与 meta 检视。
- **首次启动设置** — 语言 → 平台 → 抓包模式 →
  CA 信任 / Chromium 选择 → bot 安装 → 完成。
- **多语言** — English、日本語、繁體中文、简体中文。
  可在配置向导或侧栏即时切换，覆盖整个 UI。
- **三麻** — 完整流程：bridge、tracker、snapshot、
  分析、按模式 bot 路由、历史统计、3p uma 表。

## 支持的平台

| 平台 | 四麻 | 三麻 | AutoPlay |
|---|:---:|:---:|:---:|
| **雀魂（Mahjong Soul / Majsoul）** | &check; | &check; | （计划中） |
| **天凤（Tenhou）** | &check; | &check; | &cross; |
| **Riichi City** | （计划中） | （计划中） | &cross; |
| **Amatsuki** | （计划中） | （计划中） | &cross; |

---

## 快速开始

### A. 安装官方 Release

Akagi 以 portable zip 形式发布 — 每个平台一个自带所需文件的目录。
从 [Releases](https://github.com/shinkuan/Akagi/releases) 下载
对应操作系统的 zip,解压到任何你有写入权限的位置(例如
`~/Apps/`、桌面),然后直接运行里面的 binary 即可。配置文件、
日志、对局历史、CA 证书以及 bot 都会建立在 binary 旁边,所以
迁移 / 备份 / 卸载就是迁移 / 复制 / 删除整个目录。

| OS | 文件 | 备注 |
|---|---|---|
| Windows | `akagi-<version>-windows-x64.zip` | x86_64。需要 WebView2(Win10 1803+ 与 Win11 已预装)。SmartScreen 会警告 — 点 *More info → Run anyway*。 |
| macOS | `akagi-<version>-macos-arm64.zip` | Apple Silicon。未签名,解压后执行一次 `xattr -cr <解压后目录>`,或第一次右键 → *Open*。 |
| Linux | `akagi-<version>-linux-x64.zip` | 在 `ubuntu-22.04` 上构建(glibc 2.35+)。需要 WebKit2GTK 4.1(`apt install libwebkit2gtk-4.1-0` / `dnf install webkit2gtk4.1` / `pacman -S webkit2gtk-4.1`)。 |

每个 zip 都将 `python-build-standalone` 3.12 + `uv` 一并放在
binary 旁边,bot 开箱即用,不需要额外安装系统 Python。

首次启动时,**配置向导** 会引导你完成语言、平台、抓包
模式、可选的 bot 安装(Mortal)以及 CA 信任(仅 MITM
模式才需要)。

### B. Chromium 模式（无需信任 CA）

最简单的方式。完成配置向导后：

1. 设置 → **Capture** → 将 Mode 设为 **Chromium**。
2. 点击 **Detect** 自动查找 Chrome / Edge / Brave / Chromium，
   或手动设置 `capture.chromium.executable`。
3. Akagi 会以独立的用户配置启动浏览器，目录位于
   `<config_root>/chrome-profile`。登录雀魂后即可开始游玩。

帧通过 Chrome DevTools Protocol 拦截 — 不需要系统 proxy、
不需要证书。

### C. MITM 模式

系统级的 proxy，搭配位于 `./ca/` 的自签根 CA：

1. 在操作系统 / 浏览器的证书库中信任
   `./ca/akagi-ca.crt`（或 `.cer` / `.pem` / `.der`）。
2. 将游戏客户端的流量导向 `127.0.0.1:23410`。
   健康检查：`GET /ping` → `pong`。
3. Windows 上常用 [Proxifier](https://www.proxifier.com/)
   把指定应用程序导向 proxy。

---

## 配置文件

配置文件 `config.toml` 位于可执行文件旁（或你以 `--config`
指向的位置）。通过设置 UI 保存的修改会热重载对应子系统 —
capture / proxy / bot active 槽位无需重启整个应用即可生效。

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
executable    = ""          # 留空 = 自动检测
user_data_dir = ""          # 留空 = <config_root>/chrome-profile
start_url     = "https://game.maj-soul.com/1/"
cft_channel   = "stable"
force_cft     = false
extra_args    = []

[bot]
enabled   = true
active_4p = "mortal"        # 用于四麻
active_3p = "mortal3p"      # 用于三麻；留空 = 不启用
auto_sync = true
dir       = "./mjai_bot"
```

<details>
<summary>配置文件位置（解析顺序）</summary>

1. `--config <path>` CLI 参数。
2. `<exe_dir>/configs/config.toml`。
3. 当前工作目录下的 `./configs.toml`。
4. 以上均不存在时，首次启动会将默认值写入
   `<exe_dir>/configs/config.toml`。

旧版配置（仍使用单一 `active = "..."` 键）加载时会自动
迁移为 `active_4p`。
</details>

---

## Bots

### 安装 Bot

配置向导或 **Bots** 标签页可直接从 GitHub release 安装 bot：

- Repo：`shinkuan/Akagi-MjaiBot-Mortal`
- 4P 资源：`release4p.zip`
- 3P 资源：`release3p.zip`

IPC 命令 `install_bot_from_github(repo, asset_glob?, name?)`
会拉取最新 release zip，解压到 `mjai_bot/<name>/`，验证
`bot.py`，并执行一次 `uv sync`。后续启动很快 — sync
会根据 `mjai_bot/<name>/.akagi/synced.stamp` 戳记决定是否
跳过。

> [!IMPORTANT]
> 由于 GitHub 的文件大小限制，release zip 中附带的 Mortal
> 权重是体积很小、强度很弱的 **占位模型**，仅用于验证安装
> 是否成功，**不建议实战使用**。
> **更强的 Mortal 权重** 与 **在线 API 服务器模型**
> （托管型、强度更高的模型 — 将 bot 指向服务器并提供
> API 密钥即可，本机不需要 NN）皆通过
> [Discord 服务器](https://discord.gg/Z2wjXUK8bN) 发放。
> 请在该处申请访问；4P 与 3P 两个版本都有提供。

### 按模式切换的 bot

`bot.active_4p` 与 `bot.active_3p` 互相独立。Akagi 会在开
局时按牌桌人数选用对应的 bot。将某个槽位留空即可在该
模式下仅使用 **分析功能**（不显示 bot 建议）。

### 自行编写 bot

```
mjai_bot/<name>/
├── bot.py            # JSONL stdin → JSONL stdout
├── pyproject.toml    # requires-python = ">=3.12"
├── manifest.toml     # 可选 — supported_modes、配置 schema
└── README.md
```

`bot.py` 从 stdin 每行读取一个 mjai 事件 JSON 数组，并向
stdout 每行写出一个 mjai 动作对象（无动作时输出
`{"type":"none"}`）。Akagi 会把 stderr 内容写入应用日志
中的 `bot=<name>` 条目。

完整协议、manifest schema 以及 secret 字段处理请见
[`src/bot/README.md`](./src/bot/README.md)。
[`mjai_bot/example/`](./mjai_bot/example/) 是一个 in-tree、
可运行的规则型示例 bot。

### AGPL 边界

Bot 以 Akagi 启动的 **独立 OS 子进程** 运行。通信严格通
过 stdin / stdout 上的 JSONL 进行 — 没有 in-process 链接、
没有共享地址空间、没有 FFI。这是有意设计的许可边界：
AGPL 许可的 bot（例如链接 libriichi 的 Mortal）会留在
其自己的进程内，因此把它放入 `mjai_bot/<name>/` **不会**
让 Akagi 成为该 bot 的衍生作品。

---

## 对局历史

每一场干净结束的对局（产生了 `end_game` mjai 事件）都会
被持久化到 `<config_root>/history/`：

```
<config_root>/history/
├── index.jsonl              # 每行一条 GameRecord（以 ULID 为 key）
└── games/
    └── <ulid>.mjai.jsonl    # 完整事件流的副本
```

中途断线会在 buffer 中留下未完成的记录并被静默丢弃 —
只有完整对局会落到磁盘。

前端 **History** 标签页显示：

- **名次饼图** — 1/2/3/4 名分布（三麻只有 3 片）。
- **累计 PT 折线图** — 可选择计分规则：
  - **雀魂**：选择 `场次`（铜 / 银 / 金 / 玉 / 王座）与
    `段位`（初心 1 星 → 魂天）。
  - **天凤**：选择 `段位`（新人 → 天凤位，共 21 阶）。
  - **自定义**：直接编辑 uma 与段位奖金数组。
  切换规则 / 段位会立即重绘 — 无需 backend round-trip。
- **详细统计** — 和牌率、放铳率、立直率、副露率、
  流局率、平均和牌 / 放铳点数、平均和牌巡目、
  役满 / 流局满贯次数。
- **对局列表** — 可按平台 / 人数 / 东风或半庄 / 日期过滤。
  点击行即可看到最终排名与该局统计；垃圾桶图标会同时
  删除 index 条目与该局的 `.mjai.jsonl`。

PT 规则与过滤条件会持久化到 `localStorage`。Bridge 启动
时从 backend 加载记录，并通过 `history-recorded` Tauri
事件保持同步。

数学细节、存储 schema，以及如何新增平台 / 统计字段 /
过滤维度请见 [`src/history/README.md`](./src/history/README.md)。

---

## 日志与诊断

每次 session 的日志会落在 `<log_dir>/<YYYYMMDD-HHMMSS>/`：

```
<log_dir>/<session>/
├── all.log                       # 所有 tracing 输出汇总
├── <target>.log                  # 按模块过滤的日志
├── proxy.binlog                  # 原始 WS 二进制帧
├── majsoul/<flow_id>.log         # 每条 WebSocket flow 的 JSON 日志
├── majsoul/<flow_id>.mjai.jsonl  # 每场对局的 mjai 事件流
└── inspector.jsonl               # Inspector 看到的帧
```

前端 **Logs** 路由有两个标签页：

### Diagnostic

可过滤的应用日志。可按级别（trace / debug / info /
warn / error）与模块过滤。可实时 tail 或浏览过去的
session；点击行可看到原始结构化字段与源位置。
**Open Folder** 按钮会在系统文件管理器中打开该 session
目录。

### Inspector

协议级的帧查看器。共三类条目：

- **WS Frame** — 原始二进制（base64 截短）加上 bridge
  的初步解析结果。
- **MjaiEvent** — 流向 bot 的解码后事件。
- **BotReaction** — bot 的回应，含 `meta` 字段
  （置信度 / q-values / bot 想发送的任意信息）。

帧计数会显示每个 WS 帧产生了多少个 mjai 事件，
在排查 bot 或 bridge 问题时很有用。

---

## 疑难排查

> [!TIP]
> 复现问题后，保存 `<log_dir>/<session>/` 整个 session
> 目录 — 内含应用日志、原始帧、mjai 事件、bot meta，
> 是提交有用 bug 报告所需的全部信息。

- **MITM 模式抓不到包。** 确认 `./ca/akagi-ca.crt`
  已在系统证书库中信任。确认 proxy 已启动：
  `curl http://127.0.0.1:23410/ping` 应回应 `pong`。
  确认你的 proxy 重定向工具（Proxifier / 系统 proxy）
  正把游戏客户端送到正确的 host:port。
- **Chromium 模式抓不到包。** Detect 没找到浏览器。
  在设置或 `config.toml` 里手动设置
  `capture.chromium.executable`。如果浏览器有启动但没
  帧流入，检查 `--remote-debugging-port` 是否被其他
  扩展拦截。
- **Bot 卡在 `Loading{SyncingDeps}`。** 首次 `uv sync`
  会很慢 — 在 Diagnostic 标签页观察 `bot=<name>` 的消息。
  若一直未完成，删除
  `mjai_bot/<name>/.akagi/synced.stamp` 后重试。
- **Bot 对局途中崩溃。** Inspector 标签页可显示 bot 死前
  看到的最后一帧；附在 bug 报告里。
- **三麻挑了错的 bot。** 检查设置 → Bot 中的
  `bot.active_3p` — 它与 `bot.active_4p` 互相独立。
- **去哪求助？** 聊天请到
  [Discord](https://discord.gg/Z2wjXUK8bN)，
  追踪型的 bug 与功能建议请到
  [GitHub Issues](https://github.com/shinkuan/Akagi/issues)。

---

## Roadmap

alpha.8 已完成：

- [x] 三麻 — 完整流程
- [x] 天凤 bridge（仅观战）
- [x] 对局历史持久化 + History 标签页（名次饼图 / PT 图 / 统计）
- [x] 日志查看（Diagnostic + Inspector）
- [x] i18n：en / ja / zh-TW / zh-CN，含配置向导语言选择
- [x] 从 GitHub release 安装 bot
- [x] Chromium 抓包模式（无需信任 CA）

计划中：

- [ ] **Riichi City** 平台支持
- [ ] **Amatsuki** 平台支持
- [ ] **自定义主题**（前端 theming hook）
- [ ] **AutoPlay**（先支持雀魂；由 bot 自主控制牌桌，
      类似原版 Akagi 在 Windows 的 AutoPlay）
- [ ] **前端打磨** — 牌型布局、动画、无障碍
- [ ] **天凤 autoplay**（目前仅观战）

详细的 bug 跟踪请到
[GitHub Issues](https://github.com/shinkuan/Akagi/issues)。

---
---

## 架构

单一 Rust 可执行文件。各子系统只持有自己的 bus handle，
彼此互不拥有。
[`src/event_bus.rs`](./src/event_bus.rs) 是所有 channel
类型的单一真相来源。

```
                ┌────────────────────────┐
   游戏客户端 ─│  capture (mitm | cdp)  │── CA 位于 ./ca（仅 mitm）
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

[`src/lib.rs`](./src/lib.rs) 在启动时把这些 bus 接起来。
前端通过六个 push 事件（`mjai-event`、`bot-response`、
`bot-status`、`proxy-status`、`notify`、`history-recorded`）
与 backend 通信，pull 命令的列表请见
[`src/ipc/README.md`](./src/ipc/README.md)。

## 技术栈

| 层级 | 技术 |
|---|---|
| Shell | [Tauri](https://tauri.app) 2 |
| Backend | Rust（edition 2021）、`tokio`、`tracing`、`clap` |
| MITM | [`hudsucker`](https://crates.io/crates/hudsucker) 0.24（`rcgen-ca`、`rustls-client`） |
| CDP capture | [`chromiumoxide`](https://crates.io/crates/chromiumoxide) 0.9 |
| 麻将引擎 | [`riichienv-core`](https://github.com/smly/RiichiEnv) 0.4 |
| Protobuf | `prost` 0.14 + `prost-reflect` 0.16 |
| 前端 | [React](https://react.dev) 19、TypeScript、[Vite](https://vitejs.dev) 8 |
| 样式 | [Tailwind CSS](https://tailwindcss.com) v4、[shadcn/ui](https://ui.shadcn.com)（Radix Nova preset） |
| 状态 | [Zustand](https://github.com/pmndrs/zustand) |
| 图表 | [Recharts](https://recharts.org) |
| 牌型渲染 | [`<mah-gen>`](https://github.com/eric200203/mahgen) Web Component |
| i18n | [react-i18next](https://react.i18next.com) |
| Bot 运行环境 | `python-build-standalone` 3.12 + [`uv`](https://github.com/astral-sh/uv)（按平台打包） |

## 项目结构

```
.
├── src/
│   ├── analysis/      向听 / 听牌 / 和牌率 / 风险 / 切牌搜索
│   ├── bot/           Registry、Python runtime、JSONL 子进程执行器
│   ├── bridge/        各平台协议 → MjaiEvent
│   │   ├── majsoul/   雀魂（liqi protobuf）
│   │   └── tenhou/    天凤（JSON tag stream，仅观战）
│   ├── capture/       抓包 backend 抽象（mitm | chromium）
│   ├── config/        AppConfig（TOML）分节与解析
│   ├── event_bus.rs   子系统间的 broadcast channel
│   ├── game_state/    riichienv 驱动的镜像、snapshot、mahgen view
│   ├── history/       对局回放存储与索引
│   ├── inspector/     帧 / 事件 / bot reaction broadcaster
│   ├── ipc/           Tauri 命令、app state、capture supervisor
│   ├── logger/        每 session 日志目录与每 target 文件 appender
│   ├── proxy/         通过 hudsucker 的 MITM HTTP/HTTPS/WS；CA 位于 ./ca
│   ├── schema/        MjaiEvent enum 与 IPC payload 类型
│   └── lib.rs         启动与接线
├── mjai_bot/
│   └── example/       in-tree 规则型向听优化器
├── frontend/          React + Vite + Tailwind + shadcn UI
│   └── src/
│       ├── routes/    Overview / GameDashboard / Bots / History / Logs / Settings / Setup / InspectorView / DiagnosticView
│       ├── tiles/     仪表板磁贴（header、hands、opponents、analysis…）
│       ├── stores/    Zustand slice（game、analysis、bot、proxy、notify、layout、config）
│       └── i18n/      en / ja / zh-TW / zh-CN
├── tests/             集成测试
├── capabilities/      Tauri 权限
├── icons/             应用图标
├── tauri.conf.json    窗口与 bundle 配置
└── Cargo.toml
```

各模块的开发者指南位于对应的 `src/*/README.md`。

## 从源码构建

**前置要求**

- Rust（最新 stable，1.80+）
- Node.js 20+ 与 npm
- Tauri 2 系统依赖：
  - **Linux**：`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、
    `libayatana-appindicator3-dev`、`librsvg2-dev`、
    `protobuf-compiler`
  - **macOS**：Xcode Command Line Tools
  - **Windows**：WebView2（Windows 11 已预装）

**运行 / 构建**

```bash
# Debug — 启动 GUI;Vite dev-server 由 Tauri 代理
cargo run

# 指定配置文件路径
cargo run -- --config ./my-config.toml

# 为当前目标构建 portable zip
cargo install tauri-cli --locked          # 若尚未安装
bash scripts/fetch-runtime.sh             # 抓取 runtime/<triple>/
cargo tauri build --no-bundle             # 产出 target/<triple>/release/akagi
bash scripts/package-zip.sh <target-triple>
# → dist/akagi-<version>-<os>-<arch>.zip

# 仅启动前端 dev(Vite 在 :1420)
cd frontend && npm ci && npm run dev
```

**内置运行环境**

`scripts/fetch-runtime.sh <target-triple>` 会下载对应目标的
`python-build-standalone` 3.12 与 `uv`,并放置在 `runtime/`。
`scripts/package-zip.sh` 接着会把这个目录复制到 zip 中 binary
旁边;`src/bot/runtime.rs` 会在运行时以 exe-adjacent 的方式
找到它,因此最终的 App 即使用户没有系统 Python 也能运行。

## 测试

集成测试位于 [`tests/`](./tests/)：

| 文件 | 覆盖范围 |
|---|---|
| `analysis_pipeline.rs` | 端到端分析（事件 → 向听 → 切牌建议） |
| `analysis_bench.rs` | hot path 性能 |
| `bot_lifecycle.rs` | 安装 → sync → spawn → 来回通信 |
| `example_bot.rs` | 规则型参考 bot 跑合成对局 |
| `mortal_zip_layout.rs` | 验证 Mortal release zip 结构 |

```bash
cargo test               # 所有测试（含集成测试）
cargo test --release     # 用于性能 bench
```

## Releases 与 CI

GitHub Actions [`release.yml`](./.github/workflows/release.yml)
会在 tag 推送(`v3.*`)或手动触发时构建,每个目标产出一个
portable zip:

| OS runner | 目标 | 产出文件 |
|---|---|---|
| `ubuntu-22.04`(glibc 2.35) | `x86_64-unknown-linux-gnu` | `akagi-<version>-linux-x64.zip` |
| `macos-14` | `aarch64-apple-darwin` | `akagi-<version>-macos-arm64.zip` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `akagi-<version>-windows-x64.zip` |

每个 zip 都将 `python-build-standalone` 3.12 + `uv` 一并放在
binary 旁边,bot 不需要额外安装系统 Python 即可运行。

Tag 必须位于 `v3` 分支。

## 参考资料

| 来源 | 应用于 | 用途 |
|---|---|---|
| [mjai JSONL 规格（Gimite）](https://gimite.net/pukiwiki/index.php?Mjai%20%E9%BA%BB%E9%9B%80AI%E5%AF%BE%E6%88%A6%E3%82%B5%E3%83%BC%E3%83%90) | `src/schema/mjai/` | `MjaiEvent` enum 与 bot wire 协议 — 15 种事件、tile-string 格式、状态机规则。 |
| [`EndlessCheng/mahjong-helper`](https://github.com/EndlessCheng/mahjong-helper)（Go 分析 CLI） | `src/analysis/` | `util/` 的直接 Rust 移植 — 向听、听牌、和牌率、听牌率、风险模型、切牌搜索。 |
| [`Xerxes-2/MajsoulMax-rs`](https://github.com/Xerxes-2/MajsoulMax-rs)（Rust MITM proxy，**GPL-3.0**） | `src/proxy/handler.rs`、`src/bridge/majsoul/parser.rs`、`src/bridge/majsoul/proto/liqi.proto` | 雀魂 5 层 WS wire 格式参考（type byte → Wrapper → 内层消息 → action protobuf）。**仅参考格式 — 未复制代码。** |
| [`smly/RiichiEnv`](https://github.com/smly/RiichiEnv)（Rust RL env + Python bindings） | `Cargo.toml`（`riichienv-core` 依赖）、`src/analysis/`、`src/game_state/` | 牌 / 手牌 / 向听 / 役 / 计分原语 + 游戏状态模型。分析引擎与 game tracker 都构建在它之上。 |
| [`eric200203/mahgen`](https://github.com/eric200203/mahgen)（麻将牌渲染 DSL） | `src/game_state/mahgen_view.rs`、前端 `<mah-gen>` | DSL 语法，用于后端预先编码手牌 / 副露 / 河字符串。 |
| [`smly/mjai.app`](https://github.com/smly/mjai.app)（麻将 AI 比赛平台） | `mjai_bot/`、`src/bot/` | bot 子进程惯例 — JSONL stdin/stdout、argv `python bot.py <player_id>`、`AKAGI_PLAYER_ID` 环境变量、批次结尾 flush 点。 |
| [`shinkuan/Akagi`](https://github.com/shinkuan/Akagi)（原版 Akagi，Python） | 架构 / 行为对齐 | 我们所重现的原始功能集：MITM proxy、mjai bridge、可插拔 bot、推荐 HUD。 |

## 许可与致谢

Akagi v3 采用 [Apache License 2.0](./LICENSE.txt)。
Copyright 2026 Shinkuan。第三方致谢信息位于
[`NOTICE`](./NOTICE) — 请与许可一同阅读。按 Apache-2.0
§4(d)，再分发时必须同时附上这两个文件。

**内置 / 链接源码**

- **mahjong-helper**（MIT） — `src/analysis/` 为 `util/` 的 Rust 移植。
- **riichienv-core** / RiichiEnv（Apache-2.0） — Cargo 依赖。
- **mahgen**（MIT） — DSL + `<mah-gen>` custom element。

**仅供参考**（未复制代码；列于 `NOTICE` 以示致谢）

- **MajsoulMax-rs**（GPL-3.0） — 仅参考雀魂 WS wire 格式。
- **mjai 规格**（Gimite） — bot wire 协议。
- **mjai.app** — bot 子进程惯例。

## 鸣谢

- [Akagi](https://github.com/shinkuan/Akagi)（Python，v2）与
  [AkagiNG](https://github.com/shinkuan/AkagiNG)（Electron + Python） —
  v3 所基于的前作。
- [`mjai.app`](https://github.com/smly/mjai.app) 以及 Gimite
  制定的 mjai 规格 — 让可插拔 bot 成为可能的协议。
- [Discord](https://discord.gg/Z2wjXUK8bN) 社区提供的 bug
  报告、模型贡献与意见反馈。
