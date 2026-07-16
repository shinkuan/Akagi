<!-- markdownlint-disable MD033 MD041 -->

<br/>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/akagi-logo-dark.png">
    <img alt="Akagi" src="assets/logo/akagi-logo-light.png" width="55%">
  </picture>
</p>

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
**推荐切牌**。可执行文件本身就内置了一个AI模型 —— 无需安装任何东西 ——
它的建议会在每巡显示；若想要更强的托管模型，可以把它指向
云端推理 API。

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
- [Bots](#bots)
- [对局历史](#对局历史)
- [日志与诊断](#日志与诊断)
- [疑难排查](#疑难排查)
- [Roadmap](#roadmap)

**开发者**
- [架构](#架构)
- [技术栈](#技术栈)
- [项目结构](#项目结构)
- [mjai Bot 插件接口](#mjai-bot-插件接口)
- [从源码构建](#从源码构建)
- [测试](#测试)
- [Releases 与 CI](#releases-与-ci)
- [参考资料](#参考资料)
- [许可与致谢](#许可与致谢)
- [鸣谢](#鸣谢)

---

## 功能

- **实时 HUD** — 向听、听牌、和牌率、听牌率、对各家放铳
  风险、推荐的进攻 / 防守切牌。可拖拽、可缩放的UI布局。
- **两种抓包模式**
  - **MITM proxy**（默认） — 系统级；需一次性的 CA 信任。
  - **Chromium** — 由 Akagi 启动受控的 Chromium 系列浏览器，
    通过 Chrome DevTools Protocol 拦截 WebSocket 帧。
    无需配置 proxy 或安装证书；直接在启动的窗口中游玩即可。
- **两种 bot 后端**
  - **内置 bot**（默认） — 嵌在可执行文件内的纯 Rust 神经网络。
    不需要 Python、不需要下载、不需要配置；四麻与三麻都能直接开打。
  - **云端推理**（可选） — 把每一次决策交给通过 HTTP 访问、
    **更强的托管模型**。内置模型仍保持加载作为自动兜底，因此服务器
    连不上时也不会让对局卡住。密钥可直接在应用内购买或兑换。

  两者皆可按模式切换：`bot.active_4p` 与 `bot.active_3p`
  会按牌桌人数自动启用。
- **自动开始（雀魂）** — 在对战页（GameDashboard）控制条选择段位场的
  场次（铜 / 银 / 金 / 玉 / 王座）、人数（四人 / 三人）、场长（東 / 南）
  与目标局数，再点 **开始**，Akagi 就会替你自动排队。每局结束后自动确认
  回大厅并再次排队，打满目标局数即自动停止。需要 Chromium 抓包 backend
  且开启自动打牌（AutoPlay），且只能在大厅启动（对局中按钮会置灰）。
  回大厅用视觉指纹确认 —— 对段位場标签做 NCC 匹配 —— 因此永不盲导航，
  无法确认时会安全停止并通知。随时可停：当前对局会先打完。非 16:9 窗口下
  点击坐标会映射到居中的 16:9 游戏区，黑边不再造成自动打牌 / 自动开始的
  点击偏移。时序与阈值可在 `[autostart]` 配置节调整。
- **对局历史** — 每场结束的对局会自动记录。历史标签页显示
  名次饼图、可选计分规则的累计 PT 折线图（雀魂段位 /
  天凤段位 / 自定义 uma），以及详细统计（和牌率、放铳率、
  立直率、副露率、流局率、平均和牌 / 放铳点数、平均和牌
  巡目、役满 / 流局满贯次数）。
- **简单的首次启动设置** — 语言 → 平台 → 抓包模式 →
  CA 信任 / Chromium 选择 → bot 配置 → 完成。
- **多语言** — English、日本語、繁體中文、简体中文。
  可在配置向导或设置即时切换。
- **三麻** — 完整支持：AI分析、按模式 bot 路由、历史统计、3p uma 表。
- **应用内更新** — 启动时自动检查新版本，也可在 *设置 → 更新*
  手动检查；一键下载、原地更新并重新启动。

## 支持的平台

| 平台 | 四麻 | 三麻 | AutoPlay |
|---|:---:|:---:|:---:|
| **雀魂（Mahjong Soul / Majsoul）** | &check; | &check; | &check; |
| **天凤（Tenhou）** | &check; | &check; | &cross; |
| **Riichi City** | &check; | &check; | &cross; |
| **Amatsuki** | （计划中） | （计划中） | &cross; |

---

## 快速开始

### A. 安装官方 Release

Akagi 以 portable zip 形式发布 — 每个平台一个自带所需文件的目录。
从 [Releases](https://github.com/shinkuan/Akagi/releases) 下载
对应操作系统的 zip,解压到任何你有写入权限的位置(例如
`~/Apps/`、桌面),然后直接运行里面的`akagi`即可。配置文件、
日志、对局历史、CA 证书以及 bot 都会建立在旁边,所以
迁移 / 备份 / 卸载就是迁移 / 复制 / 删除整个目录。

| OS | 文件 | 备注 |
|---|---|---|
| Windows | `akagi-<version>-windows-x64.zip` | x86_64。需要 WebView2(Win10 1803+ 与 Win11 已预装)。SmartScreen 会警告 — 点 *More info → Run anyway*。 |
| macOS | `akagi-<version>-macos-arm64.zip` | Apple Silicon。未签名,解压后执行一次 `xattr -cr <解压后目录>`,或第一次右键 → *Open*。 |
| Linux | `akagi-<version>-linux-x64.zip` | 在 `ubuntu-22.04` 上构建(glibc 2.35+)。需要 WebKit2GTK 4.1(`apt install libwebkit2gtk-4.1-0` / `dnf install webkit2gtk4.1` / `pacman -S webkit2gtk-4.1`)。 |

首次启动时，**配置向导** 会引导你完成语言、平台、抓包模式、
bot 配置，以及 CA 信任（仅 MITM 模式才需要）。没有 bot 要安装
—— 内置的那个本来就在。

### B. Chromium 模式（无需信任 CA）

最简单的方式。完成配置向导后Akagi会自动查找 Chrome / Edge / Brave / Chromium 然后以独立的用户配置启动浏览器，登录雀魂后即可开始游玩。

帧通过 Chrome DevTools Protocol 拦截 — 不需要系统 proxy、
不需要证书。

### C. MITM 模式

系统级的 proxy，搭配位于 `./ca/` 的自签根 CA：

1. 信任证书
   `./ca/akagi-ca.crt`（或 `.cer` / `.pem` / `.der`）。
2. 将游戏客户端的流量导向 `127.0.0.1:23410`。
   健康检查：`GET /ping` → `pong`。
3. Windows 上常用 [Proxifier](https://www.proxifier.com/)
   把指定应用程序导向 proxy。
4. **把 loopback 排除在重定向之外。** `localhost`、`127.0.0.1`、`::1`
   一律走 Direct，不要经过 Akagi。

> [!IMPORTANT]
> 第 4 步不是可选的。游戏会通过 loopback 跟自己通信来处理内部事务，
> 而「匹配游戏程序、目标为任意 host」的重定向规则会把这些 socket
> 一并扫进 Akagi。Akagi 会拒绝它们（日志里会出现
> `refusing CONNECT to loopback` 警告），但游戏仍可能出问题 —
> 所以请从源头排除 loopback。
>
> Proxifier 的做法：**Profile → Proxification Rules**，启用内建的
> **Localhost** 规则（Action: *Direct*），并把它拖到游戏规则的**上面**。
> 顺序很重要 — Proxifier 只采用第一条命中的规则，Localhost
> 规则排在游戏规则下面就永远不会生效。

---

## Bots

### 内置 bot

Akagi 内置一个 **纯 Rust 的 bot**，它是两种模式的默认值（`bot.active_4p = "akagi-native"`、
`bot.active_3p = "akagi-native3p"`），会出现在 **Bots** 标签页最上方，
状态永远是「就绪」。

它是一个以行为克隆（behavior cloning）训练出来的小型神经
网络（权重直接嵌在可执行文件内），因此棋力 **刻意保持在中等水平** ——
它是个合理的默认值，而不是顶尖引擎。

### 云端推理

内置 bot 可以选择把决策交给 **远程推理服务器**，而不是运行内嵌
的模型 —— 那是一个通过网络访问、更强的托管模型。内嵌的本地模型仍会
保持加载作为自动 **兜底**：当服务器连不上、被限流，或密钥无效时，bot
会改用本地模型的着法，让进行中的对局不会卡住。

#### 获取云端推理密钥

三种方式：

- **购买密钥** — 应用内购买。
- **兑换码** — 把预付码换成密钥，或给你已持有的密钥加时间。
- 到 [Discord 服务器](https://discord.gg/Z2wjXUK8bN) 询问。

### 按模式切换的 bot

`bot.active_4p` 与 `bot.active_3p` 互相独立。Akagi 会在开
局时按牌桌人数选用对应的 bot。

除了这两种后端之外，Akagi 也能以子进程运行 **外部 mjai bot**。
那是给开发者的扩展点，而不是任何人都得走的步骤 ——
请见 [mjai Bot 插件接口](#mjai-bot-插件接口)。

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
- **MITM 模式下游戏卡在加载画面。** 多半是重定向工具把游戏的 loopback
  流量也送进了 proxy。在日志里找 `refusing CONNECT to loopback`，
  然后排除 `localhost`、`127.0.0.1`、`::1` — 见上方 MITM 设置第 4 步。
- **Chromium 模式抓不到包。** Detect 没找到浏览器。
  在设置或 `config.toml` 里手动设置
  `capture.chromium.executable`。如果浏览器有启动但没
  帧流入，检查 `--remote-debugging-port` 是否被其他
  扩展拦截。
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
- [x] Riichi City bridge（仅 MITM — 原生客户端；仅观战）
- [x] 对局历史持久化 + History 标签页（名次饼图 / PT 图 / 统计）
- [x] 日志查看（Diagnostic + Inspector）
- [x] i18n：en / ja / zh-TW / zh-CN，含配置向导语言选择
- [x] 从 GitHub release 或本地 ZIP 文件安装 bot
- [x] Chromium 抓包模式（无需信任 CA）
- [x] **自定义主题**（前端 theming hook）
- [x] **AutoPlay**（先支持雀魂；由 bot 自主控制牌桌）
- [x] **自动开始**（雀魂段位场自动排队：排队 → 打牌 → 确认回大厅 →
      再次排队，直到目标局数）

计划中：

- [ ] **Amatsuki** 平台支持
- [ ] **前端打磨** — 牌型布局、动画、无障碍
- [ ] **天凤 autoplay**

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
  analysis::runner   内置 NN（进程内）     Tauri webview
       │             | 云端 API
       ▼ AnalysisBus  | mjai 子进程
       └──► ipc forwarder ──► app.emit
```

[`src/lib.rs`](./src/lib.rs) 在启动时把这些 bus 接起来。
前端通过 push 事件（`mjai-event`、`bot-response`、
`bot-status`…）与 pull 命令和 backend 通信，两者的列表
都在 [`src/ipc/README.md`](./src/ipc/README.md)。开启
AutoPlay 时，`autoplay` manager 会取用 bot 的决策，并通过
Chromium 抓包 backend（CDP）点击牌桌。

## 技术栈

| 层级 | 技术 |
|---|---|
| Shell | [Tauri](https://tauri.app) 2 |
| Backend | Rust（edition 2021）、`tokio`、`tracing`、`clap` |
| MITM | [`hudsucker`](https://crates.io/crates/hudsucker) 0.24（`rcgen-ca`、`rustls-client`） |
| CDP capture | [`chromiumoxide`](https://crates.io/crates/chromiumoxide) 0.9 |
| 麻将引擎 | [`riichienv-core`](https://github.com/smly/RiichiEnv) 0.4 |
| 内置 bot | [`candle`](https://github.com/huggingface/candle) 0.9（纯 Rust NN 推理；权重内嵌） |
| 云端推理 | [`reqwest`](https://crates.io/crates/reqwest) 0.13（rustls） |
| Protobuf | `prost` 0.14 + `prost-reflect` 0.16 |
| 前端 | [React](https://react.dev) 19、TypeScript、[Vite](https://vitejs.dev) 8 |
| 样式 | [Tailwind CSS](https://tailwindcss.com) v4、[shadcn/ui](https://ui.shadcn.com)（Radix Nova preset） |
| 状态 | [Zustand](https://github.com/pmndrs/zustand) |
| 图表 | [Recharts](https://recharts.org) |
| 牌型渲染 | [`<mah-gen>`](https://github.com/eric200203/mahgen) Web Component |
| i18n | [react-i18next](https://react.i18next.com) |
| mjai bot 运行环境 | `python-build-standalone` 3.12 + [`uv`](https://github.com/astral-sh/uv)（按平台打包；仅插件 bot 需要 —— 内置 bot 完全用不到） |

## 项目结构

```
.
├── src/
│   ├── analysis/      向听 / 听牌 / 和牌率 / 风险 / 切牌搜索
│   ├── autoplay/      bot 决策 → 通过 CDP 点击牌桌（AutoPlay）
│   │   └── majsoul/   lobby_coords.rs、vision.rs —— 自动开始回大厅检测
│   ├── autostart.rs   段位场自动排队循环（雀魂；驱动 AutoPlay）
│   ├── bot/           Bot manager：内置 bot、云端 API client、mjai 子进程执行器
│   ├── bridge/        各平台协议 → MjaiEvent
│   │   ├── majsoul/   雀魂（liqi protobuf）
│   │   ├── riichi_city/  Riichi City（仅 MITM）
│   │   └── tenhou/    天凤（JSON tag stream，仅观战）
│   ├── capture/       抓包 backend 抽象（mitm | chromium）
│   ├── config/        AppConfig（TOML）分节与解析（含 autostart.rs）
│   ├── event_bus.rs   子系统间的 broadcast channel
│   ├── game_state/    riichienv 驱动的镜像、snapshot、mahgen view
│   ├── github/        GitHub Releases client（bot 安装、自我更新）
│   ├── history/       对局回放存储与索引
│   ├── inspector/     帧 / 事件 / bot reaction broadcaster
│   ├── ipc/           Tauri 命令、app state、capture supervisor
│   ├── logger/        每 session 日志目录与每 target 文件 appender
│   ├── proxy/         通过 hudsucker 的 MITM HTTP/HTTPS/WS；CA 位于 ./ca
│   ├── schema/        MjaiEvent enum 与 IPC payload 类型
│   ├── updater/       应用内自我更新（检查 + 应用）
│   └── lib.rs         启动与接线
├── native_bot/        内置 bot crate：obs/action codec、candle CNN、内嵌权重
├── mjai_bot/
│   └── example/       in-tree 规则型向听优化器
├── frontend/          React + Vite + Tailwind + shadcn UI
│   └── src/
│       ├── routes/    Overview / GameDashboard / Bots / History / Logs / Settings / Setup / InspectorView / DiagnosticView
│       ├── tiles/     仪表板磁贴（header、hands、opponents、analysis…）
│       ├── stores/    Zustand store，一个领域一个（game、bot、config、theme…）
│       └── i18n/      en / ja / zh-TW / zh-CN
├── tests/             集成测试
├── capabilities/      Tauri 权限
├── icons/             应用图标
├── tauri.conf.json    窗口与 bundle 配置
└── Cargo.toml
```

各模块的开发者指南位于对应的 `src/*/README.md`。

## mjai Bot 插件接口

> 可选功能，主要面向开发者。[内置 bot](#内置-bot) 才是默认值，完全不需要
> 这一节的任何步骤 —— 只有当你想让 Akagi 驱动 *另一个* 引擎时才会用到。

除了自家的 bot 之外，Akagi 也能驱动任何遵循 **mjai** 协议的引擎。这种 bot
是一个独立子进程，通过 stdin/stdout 以 JSONL 通信：Akagi 把对局以 mjai
事件喂给它，它则回复一个动作，以及可选的 HUD 数据。

### 自行编写

```
mjai_bot/<name>/
├── bot.py            # JSONL stdin → JSONL stdout
├── pyproject.toml    # requires-python = ">=3.12"
├── manifest.toml     # 可选 — supported_modes、配置 schema
└── README.md
```

`bot.py` 从 stdin 每行读取一个 mjai 事件 JSON 数组，并向 stdout 每行写出
一个 mjai 动作对象（无动作时输出 `{"type":"none"}`）。Akagi 会把 stderr
内容写入应用日志中的 `bot=<name>` 条目。

完整的 I/O 协议、mjai 事件流、reaction 与 `meta` HUD 格式、toast 通知，
以及 `manifest.toml` 配置，请见
**[`mjai_bot/README.md`](./mjai_bot/README.md)**。
[`mjai_bot/example/`](./mjai_bot/example/) 是一个可直接复制、可运行的
规则型示例 bot。

本地开发时，把 bot 文件夹放到 `mjai_bot/<name>/`，在 **Bots** 标签页该 bot
行上点击 **安装环境** 即可构建其 venv —— 无需每次改动都重新打包安装。
环境就绪前，启用开关会保持禁用。

### 安装

**Bots** 标签页可以从 GitHub release 或本地 ZIP 安装 bot。

IPC 命令 `install_bot_from_github(repo, asset_glob?, name?)` 会拉取最新
release zip，解压到 `mjai_bot/<name>/`，验证 `bot.py`，并执行一次
`uv sync`。后续启动很快 —— sync 会根据
`mjai_bot/<name>/.akagi/synced.stamp` 戳记决定是否跳过。

**从 ZIP 安装** 是离线的等价流程：点击 **浏览…** 选择 `.zip`（或粘贴其
路径）即可。它执行完全相同的解压 / 验证 / `uv sync` 流程，并且不会改动
你的源 `.zip`。

### AGPL 边界

Bot 以 Akagi 启动的 **独立 OS 子进程** 运行。通信严格通过 stdin / stdout
上的 JSONL 进行 —— 没有 in-process 链接、没有共享地址空间、没有 FFI。
这是有意设计的许可边界：AGPL 许可的 bot（例如链接 libriichi 的 Mortal）
会留在其自己的进程内，因此把它放入 `mjai_bot/<name>/` **不会** 让 Akagi
成为该 bot 的衍生作品。

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
