<div align="center">
  <img src="https://github.com/shinkuan/RandomStuff/assets/35415788/db94b436-c3d4-4c57-893e-8db2074d2d22" width="50%" alt="Akagi Shigeru">
  <h1>Akagi-NG</h1>

  <p>
    Next Generation Mahjong AI Assistant<br>
    Inspired by <b>Akagi</b> and <b>MajsoulHelper</b>
  </p>
<p><i>「死ねば助かるのに……」— 赤木しげる</i></p>

<p>
<a href="https://github.com/shinkuan/Akagi/actions/workflows/test.yml"><img src="https://img.shields.io/github/actions/workflow/status/shinkuan/Akagi/test.yml?branch=ng&label=CI&labelColor=181717&logo=github" alt="CI Status"></a>
<a href="https://github.com/shinkuan/Akagi/releases"><img src="https://img.shields.io/github/v/release/shinkuan/Akagi?labelColor=181717&logo=github&display_name=tag" alt="GitHub release"></a>
<a href="https://github.com/shinkuan/Akagi/stargazers"><img src="https://img.shields.io/github/stars/shinkuan/Akagi?style=social" alt="GitHub stars"></a>
<br>
<img src="https://img.shields.io/badge/Electron-47848F?logo=electron&logoColor=white" alt="Electron">
<img src="https://img.shields.io/badge/React-20232A?logo=react&logoColor=61DAFB" alt="React">
<img src="https://img.shields.io/badge/TypeScript-3178C6?logo=typescript&logoColor=white" alt="TypeScript">
<img src="https://img.shields.io/badge/Vite-9135FF?logo=vite&logoColor=white" alt="Vite">
<img src="https://img.shields.io/badge/Tailwind_CSS-06B6D4?logo=tailwind-css&logoColor=white" alt="Tailwind CSS">
<img src="https://img.shields.io/badge/Python-3776AB?logo=python&logoColor=white" alt="Python">
<img src="https://img.shields.io/badge/PyTorch-EE4C2C?logo=pytorch&logoColor=white" alt="PyTorch">
<br>
<img src="https://img.shields.io/badge/License-AGPL--3.0%20+%20Commons%20Clause-663366?labelColor=808080" alt="License">
<a href="https://discord.gg/Z2wjXUK8bN"><img src="https://img.shields.io/discord/1192792431364673577?label=Discord&labelColor=5865F2&logo=discord&logoColor=white" alt="Discord"></a>
<a href="https://codecov.io/gh/shinkuan/Akagi"><img src="https://img.shields.io/codecov/c/github/shinkuan/Akagi/ng?labelColor=F01F7A&logo=Codecov&logoColor=white" alt="Codecov"></a>
</p>

<p align="center">
  <b>简体中文</b> | <a href="./README.md">English</a>
</p>
</div>

---

## 什么是 Akagi-NG？

**Akagi-NG** 是原 **Akagi** 项目的次世代版本。

这是一款专为日本麻将（立直麻将）设计的 AI 辅助工具，旨在为线上麻将游戏对局提供实时局势分析与决策建议。

Akagi-NG 的核心理念：

- **现代化架构**：基于 Electron 原生桌面架构，使用 Python 3.12 与 React/Vite 重写
- **解耦合设计**：核心逻辑、用户界面、配置管理与 AI 模型彻底分离
- **高性能推理**：集成 `libriichi` 获取极速的 Mortal 模型推理能力
- **长期可维护性**：优化的代码结构便于持续迭代

---

## 核心架构

下面这张图展示了 Akagi-NG 在 Electron 桌面 / MITM 两种模式下的整体数据流。

**组件说明：**

- **双模式拦截层**：提供两种游戏数据获取方式
  - **桌面模式**：通过内置 Chromium 浏览器 + 资源钩子直接截获游戏通信，无需配置代理或证书
  - **MITM 模式**：通过 mitmproxy 代理拦截外部浏览器/客户端流量，支持第三方设备

- **Python 后端 (DataServer)**：数据处理中枢
  - **协议桥接**：将雀魂协议转换为 MJAI 标准格式
  - **状态跟踪 Bot**：维护完整的游戏状态机
  - **AI 控制器**：管理 AI 模型和 Lookahead 前瞻算法，提供决策建议
  - **SSE 事件流**：通过 Server-Sent Events 实时推送 AI 决策到前端

- **Electron 前端**：用户界面和窗口管理
  - **主进程**：负责后端进程管理和多窗口协调
  - **控制面板**：提供配置管理和游戏启动入口
  - **HUD 覆盖层**：透明叠加窗口，实时显示 AI 推荐信息

- **游戏窗口**：最终的游戏呈现层
  - 桌面模式下由 Electron 管理的 Chromium 实例
  - MITM 模式下为用户自行打开的浏览器或游戏客户端

**数据流概览：**

1. 游戏服务器 → 拦截层：原始游戏数据通过 WebSocket 传输
2. 拦截层 → Python 后端：转换为 MJAI 事件格式
3. Python 后端处理：状态跟踪 + AI 决策计算
4. Python 后端 → Electron：通过 SSE 推送决策结果
5. HUD 覆盖层：在游戏窗口上叠加显示 AI 建议

```mermaid
graph TB
    %% 游戏服务器
    GS["游戏服务器<br/>"]

    %% 双模式入口
    subgraph Modes ["双模式拦截"]
        direction LR
        DesktopMode["桌面模式<br/>内置 Chromium + Hook"]
        MitmMode["MITM 模式<br/>mitm-proxy"]
    end

    %% Python 后端核心
    subgraph Backend ["Python 后端"]
        direction TB
        Bridge["协议桥接<br/>(MJAI Adapter)"]
        Bot["状态跟踪 Bot<br/>(State Tracker)"]
        Controller["AI 控制器<br/>(Mortal + Lookahead)"]
        SSE["SSE 事件流<br/>(Real-time Push)"]

        Bridge --> Bot
        Bridge --> Controller
        Bot --> SSE
        Controller --> SSE
    end

    %% Electron 前端
    subgraph Frontend ["Electron 前端"]
        direction TB
        MainProcess["主进程<br/>(Backend Manager + Window Manager)"]
        Dashboard["控制面板<br/>(Dashboard)"]
        HUD["HUD 覆盖层<br/>(Transparent Overlay)"]

        MainProcess -- "窗口管理" --> Dashboard
        MainProcess -- "窗口管理" --> HUD
    end

    %% 游戏窗口
    GameWin["游戏窗口<br/>(Chromium Instance or Client)"]

    %% 数据流
    GS <-- "游戏数据" --> DesktopMode
    GS <-- "游戏数据" --> MitmMode

    DesktopMode -- "MJAI 事件" --> Bridge
    MitmMode -- "MJAI 事件" --> Bridge

    SSE -- "SSE 连接" --> Dashboard
    SSE -- "AI 决策" --> HUD

    Dashboard -- "IPC" --> MainProcess
    MainProcess -- "窗口管理 (仅桌面模式)" --> GameWin
    HUD -- "视觉叠加" --> GameWin

    %% 样式
    classDef serverStyle fill:#fce4ec,stroke:#c2185b,stroke-width:2px
    classDef modeStyle fill:#e3f2fd,stroke:#1976d2,stroke-width:2px
    classDef backendStyle fill:#f3e5f5,stroke:#7b1fa2,stroke-width:2px
    classDef frontendStyle fill:#e8f5e9,stroke:#388e3c,stroke-width:2px
    classDef gameStyle fill:#fff3e0,stroke:#ef6c00,stroke-width:3px

    class GS serverStyle
    class DesktopMode,MitmMode modeStyle
    class Bridge,Bot,Controller,SSE backendStyle
    class MainProcess,Dashboard,HUD frontendStyle
    class GameWin gameStyle
```

---

## 功能特性

- 🎮 **支持平台**
  - 雀魂麻将 (Mahjong Soul)
  - 天凤 (Tenhou)
  - 麻雀一番街 (Riichi City)
  - 天月麻雀 (Amatsuki Mahjong)

- 🀄 **支持模式**
  - 四人麻将 (4p)
  - 三人麻将 (3p)

- 🤖 **AI模型**
  - Mortal (Mortal 4p / Mortal 3p)
  - AkagiOT (AkagiOT 4p / AkagiOT 3p)

- 🧠 **核心功能**
  - 实时手牌分析与 AI 何切推荐
  - 立直模拟推演 - 智能推荐最佳立直舍牌
  - 完整的副露支持 - 吃/碰/大明杠/暗杠/加杠操作提示一目了然
  - 全新毛玻璃 HUD 界面 - 丝滑且无感的悬浮窗体验
  - 多语言支持 - 简体中文 / 繁體中文 / 日本語 / English

> [!NOTE]
> **Riichi Lookahead（立直模拟推演）** 是 Akagi-NG 的一项核心功能，旨在解决“当 AI 建议立直时，我应该切哪张牌？”的问题。
>
> <details>
> <summary><b>点击查看立直模拟推演的详细逻辑</b></summary>
>
> **1. 为什么需要它？**
>
> 当 MJAI 引擎（如 Mortal）建议执行 `立直` 操作时，协议返回的动作仅仅是 `{"type": "reach"}`。它**不会**直接告诉我们立直后应该切哪张牌（例如 `6m`）。
> 然而，对于用户来说，点击“立直”按钮后，下一步必须切出一张牌。如果没有 Lookahead，用户只能瞎猜或者自己判断切哪张，这可能会导致 AI 建议的立直策略无法正确执行（例如切了错误的牌导致振听或放铳）。
>
> **2. 工作原理**
>
> Lookahead 的核心思想是**“模拟未来”**。当 AI 建议立直时，我们创建一个临时的平行宇宙，假设玩家已经声明了立直，然后问 AI 在那个状态下会切什么牌。
>
> 流程分为以下几步：
>
> 1. **触发**：当前局面下，AI 引擎经过推理，认为 `立直 (riichi)` 是前 5 名的最佳动作之一。
> 2. **启动模拟**：Akagi-NG 创建一个新的、临时的 `Simulation Bot`（模拟机器人）。
> 3. **历史重放 (History Replay)**：
>    - 为了让模拟机器人达到当前的游戏状态，我们需要把从开局到现在发生的所有事件（摸牌、打牌、吃碰杠等）全部喂给它一遍。
>    - **旧机制**：在重放每一手“自己的动作”时，模拟机器人都会傻乎乎地去问 AI 引擎：“这时候我该干嘛？”。这导致了一局游戏进行到第 15 巡时，Lookahead 需要进行 15 次以上的 AI 推理。对于 Online 模式，这就是 15 次 HTTP 请求，瞬间触发 429 封禁。
>    - **新机制 (ReplayEngine)**：我们现在使用 ReplayEngine 包装器。在重放阶段，我们明确知道这只是在“复述历史”，所以当模拟机器人问“这时候我该干嘛？”时，ReplayEngine 直接返回一个哑动作（如 `摸切 (tsumogiri)`），**完全跳过 AI 推理**。这使得重放过程几乎是瞬时的，且零网络消耗。
> 4. **分支 (Branching)**：
>    - 当状态完全恢复到“现在”后，我们手动向模拟机器人发送一个 `立直 (riichi)` 事件。
>    - 此时，模拟机器人的内部状态就变成了：“玩家刚刚宣布了立直，正等待切牌”。
> 5. **最终推理 (Final Inference)**：
>    - 在这个“宣布立直”的新状态下，我们向 AI 引擎发起一次**真实**的询问：“现在最佳切牌是什么？”
>    - 引擎会根据局面分析，返回具体的切牌动作（例如 `打 6m`）。
> 6. **结果展示**：前端 UI 接收到这个`6m`的信息，界面上会既会高亮显示立直和其他切牌推荐（比如`默听 (damaten)`），也会在立直推荐的子条目展示建议切出的那张`6m`。若立直切牌候选多于1种，子条目中会分别展示每张立直切牌和置信度。
>
> </details>

## 演示视频

https://github.com/user-attachments/assets/701a3dcf-1574-46af-9594-082605c4e158

## 运行截图

### 主界面

![主界面](./docs/screen_shots/ui.png)

### 设置面板

![设置面板](./docs/screen_shots/settings_panel.png)

---

## ⚠️ 免责声明

本项目**仅供教育及研究使用**。

在网络游戏中使用第三方辅助工具可能违反游戏的服务条款。
Akagi-NG 的作者及贡献者**不对任何使用后果负责**，包括但不限于**账号被封禁或冻结**。

## 请您在使用前充分了解并自行承担相关风险。

## 安装与使用

### 1. 快速开始

1. **下载**: 前往 [Releases](../../releases) 下载最新版本的压缩包并解压。
2. **部署**: 将 AI 模型权重文件 (`.pth`) 放入 `models/` 目录，二进制扩展 (`.pyd`/`.so`) 放入 `lib/` 目录。
3. **运行**: 双击运行 `Akagi-NG.exe`。
4. **对局**: 在 Dashboard 中点击“**启动游戏**”，点击右上角显示器图标开启 **HUD**。

### 2. 目录结构说明

为了确保程序正常运行，请检查 `Akagi-NG.exe` 所在目录结构是否完整：

```plain
Akagi-NG/
  ├── Akagi-NG.exe     # 主程序 (Electron 桌面端)
  ├── assets/          # 各平台相关的界面资源
  ├── bin/             # 后端核心程序所在的目录
  ├── config/          # 配置文件目录 (settings.json)
  ├── lib/             # libriichi 二进制扩展 (.pyd/.so)
  │     ├── libriichi.pyd
  │     └── libriichi3p.pyd
  ├── locales/         # 多语言支持资源
  ├── logs/            # 运行日志目录
  ├── models/          # AI 模型权重文件 (.pth)
  │     ├── mortal.pth
  │     └── mortal3p.pth
  ├── resources/       # 应用程序核心资源 (app.asar)
  ├── LICENSE.txt      # 开源许可协议
  ├── README.txt       # 极简使用说明
  └── ...              # 其他必要的运行时支持文件 (.dll, .pak 等)
```

### 3. 启动与退出

双击运行 `Akagi-NG.exe` 后，程序将展示集成化的 Dashboard 面板。您可以直接点击 Dashboard 中的“开始游戏”启动游戏浏览器窗口。

点击 Dashboard 右上角的显示器图标即可开启 HUD 界面。

如需退出程序，请点击 Dashboard 右上角的红色电源图标。

> [!TIP]
> **HUD (Heads-Up Display)** 是 Akagi-NG 的一项核心特性。它能够将辅助信息直接以半透明形式覆盖在游戏画面上，无需手动置顶窗口。

### 4. 配置

Akagi-NG 的所有配置均位于 `config/settings.json` 文件中。您可以点击 Dashboard 右上角的齿轮图标进入设置面板来修改，也可以使用文本编辑器修改此文件来调整程序行为。

### 5. 桌面模式 (推荐)

这是 Akagi-NG的**默认工作模式**。

在此模式下，Akagi-NG 利用 Electron 核心管理一个专用的 Chromium 实例来运行游戏。

- **核心优势**：
  - **免配置**：无需证书或代理设置，一键启动。
  - **环境隔离**：与您日常使用的浏览器完全隔离，互不干扰。
  - **安全稳定**：直接从游戏服务器接收数据，稳定性高。

- **使用方法**：
  1. 运行 `Akagi-NG.exe`。
  2. 在 Dashboard 中点击“启动游戏”。

### 6. MITM 模式

Akagi-NG 支持通过中间人攻击 (MITM) 方式截获游戏数据，这允许您使用任意浏览器、游戏客户端或移动设备（配合代理）进行对局。

1. **启用配置**:
   在设置面板中启用“外部代理”或在 `config/settings.json` 中手动修改 `mitm` 字段：

   ```yaml
   "mitm": { "enabled": true, "host": "127.0.0.1", "port": 6789 }
   ```

2. **设置代理**:
   将您的浏览器或系统代理设置为 `127.0.0.1:6789`。

3. **安装证书**:

   > 注意：如果您使用了方案 B (Clash 分流)，可能无法打开 `mitm.it`。推荐使用**方法二**。
   - **方法一：在线安装 (推荐方案 A 用户)**
     - 启动 Akagi-NG。
     - 访问 [http://mitm.it](http://mitm.it)。
     - 下载 Windows 证书 (p12 或 cer)。

   - **方法二：本地安装 (推荐方案 B 用户)**
     - 找到用户目录下的 `.mitmproxy` 文件夹 (例如 `C:\Users\<YourName>\.mitmproxy`)。
     - 双击 `mitmproxy-ca-cert.p12` 进行安装。

   - **关键步骤**：
     - 双击证书 -> 安装证书 -> 存储位置选“**受信任的根证书颁发机构**” (Trusted Root Certification Authorities)。

> [!WARNING]
> 务必将证书安装到“**受信任的根证书颁发机构**” (Trusted Root Certification Authorities)。

### 7. 常见问题

**Q: 第一次启动程序，界面卡在欢迎/加载界面怎么办？**
**A:** 这种情况通常是由于 DataServer 默认端口 `8765` 被其他程序占用（冲突）导致的。
**解决方法：**

1. 打开 `config/settings.json`。
2. 找到 `"server": { "port": 8765 }`，将 `8765` 修改为其他未占用的端口（如 `8888`）。
3. 重启程序。

**Q: 我正在使用 Clash/v2rayN 等代理软件 (TUN/系统代理模式)，如何配置？**

<details>
<summary><b>点击查看详细代理分流配置方案</b></summary>

#### 配置方案 A: 浏览器网页版 (SwitchyOmega 代理)

此方案适合**网页版**玩家，配置最简单且完全隔离。

**配置步骤 (以 Clash Verge Tun 模式为例)**：

1. **准备环境**：
   - 保持 Clash Verge 的 Tun 模式 **开启**。
   - 确保 Akagi-NG 已启动且 `mitm.enabled` 为 `true` (端口默认 6789)。

2. **安装 SwitchyOmega**:
   - Chrome/Edge 用户请在商店搜索并安装 "SwitchyOmega"。

3. **配置情景模式**:
   - 打开 SwitchyOmega 设置界面。
   - 点击左侧 **"新建情景模式"** -> 命名为 `Akagi-Mitm` -> 类型选择 **"代理服务器"**。
   - 在 `Akagi-Mitm` 的设置中：
     - 协议：`HTTP`
     - 服务器：`127.0.0.1`
     - 端口：`6789`
   - 点击左侧 **"应用选项"** 保存。

4. **配置自动切换 (关键)**:
   - 点击左侧 **"自动切换"** (Auto Switch)。
   - 删除所有现有规则（如果有）。
   - **添加规则**：
     - 规则匹配：`*.maj-soul.com  ->  Akagi-Mitm`
     - 规则匹配：`*.majsoul.com  ->  Akagi-Mitm`
     - 规则匹配：`*.mahjongsoul.com  ->  Akagi-Mitm`
   - **默认规则** (Default)：
     - 选择 **"直接连接"** (Direct)，然后点击 **"应用选项"** 保存。

> [!TIP]
> 因为您的系统已经开启了 Tun 模式，"直连"的流量会自动被 Tun 网卡接管并代理，所以不需要在这里选"系统代理"或"Clash"。

> [!IMPORTANT]
> 如果您没开 Tun 模式，仅开了系统代理，这里请选"系统代理"

5. **开始游戏**:
   - 点击浏览器右上角 SwitchyOmega 图标，选择 **"自动切换"**。
   - 访问雀魂网页版，Akagi-NG 应该能正常获取游戏事件，同时您依然可以访问 Google/YouTube (走 Tun)。

#### 配置方案 B: 雀魂客户端 (Clash 规则分流)

此方案适合**PC/Steam客户端**玩家。由于客户端无法像浏览器那样使用插件分流，我们需要直接修改 Clash 配置，让它把游戏流量转发给 Akagi-NG。

> [!IMPORTANT]
> 使用PC/Steam客户端游玩时，请确保 Clash 处在 TUN 模式下，否则将无法代理客户端流量

1. **找到配置入口**:
   - 在 Clash Verge 中找到您的配置文件（或者使用 "Merge" / "Script" 功能进行注入，以免覆盖原配置）。

2. **添加代理节点 (Proxies)**:
   定义一个指向 Akagi-NG 本地代理的节点。

   ```yaml
   proxies:
     - name: Akagi-Mitm
       type: http
       server: 127.0.0.1
       port: 6789
       tls: false
   ```

   还可以定义一个代理组 (Proxy-groups)，里面包含本地代理节点和直连 (Direct)，这样方便切换是否使用 Akagi-NG 本地代理。

   ```yaml
   proxy-groups:
     - name: 🀄 雀魂麻将
       proxies:
         - Akagi-Mitm
         - DIRECT
       type: select
   ```

3. **添加分流规则 (Rules)**:
   将雀魂相关域名强制指向上面定义的代理节点。请注意规则顺序，建议将规则放在靠前位置。

   ```yaml
   rules:
     - PROCESS-NAME,akagi-ng.exe,DIRECT
     - PROCESS-NAME,雀魂麻將,🀄 雀魂麻将
     - PROCESS-NAME,Jantama_MahjongSoul.exe,🀄 雀魂麻将
     - DOMAIN-Keyword,maj-soul,🀄 雀魂麻将
   ```

4. **应用配置**:
   保存并刷新 Clash 配置。现在启动雀魂客户端，流量路径为：
   `雀魂客户端 -> Clash (TUN) -> 匹配到 Rules -> 转发给 Akagi-NG (6789) -> 您的网络/上游代理`

</details>

---

## 源码构建指南

### 环境要求

- **Python 3.12+**: 用于运行后端推理引擎。
- **Node.js 24+ & npm**: 用于编译 Electron 桌面端和 React 前端。
- **Git**: 用于克隆项目仓库。
- **Windows**: 推荐的开发与构建平台。

### 1. 项目初始化

克隆仓库并进入目录：

```bash
git clone -b ng https://github.com/shinkuan/Akagi.git
cd Akagi
```

#### 后端环境准备

集成构建脚本依赖于 `akagi_backend` 目录下的虚拟环境：

```bash
cd akagi_backend
python -m venv .venv
# Windows
.\.venv\Scripts\activate
# Linux/macOS
source .venv/bin/activate

pip install -e .[dev]
cd ..
```
#### Libriichi 初始化

在`lib`目录中，将对应平台的`.pyd/.so`复制为对应的`libriichi[3p].{pyd|so}`。

```bash
cd lib
# Windows
cp libriichi-*.pyd libriichi.pyd
cp libriichi3p-*.pyd libriichi3p.pyd

cd ..
```

#### 前端与桌面端初始化

```bash
# 安装前端依赖
cd akagi_frontend
npm install

# 安装 Electron 桌面端依赖
cd ../electron
npm install
```

### 2. 开发环境运行

在 `electron` 目录下执行启动命令：

```bash
cd electron
npm run dev
```

### 3. 生产环境构建

执行以下命令打包对应当前平台的安装包：

```bash
cd electron
npm run build
```

构建产物将生成于 `dist/release` 目录下。

> [!IMPORTANT]
> 构建过程仅打包应用程序主体。为了使程序正常工作，您仍需确保将 AI 模型权重文件 (`.pth`) 放置在 `models/` 目录，并将二进制扩展 (`.pyd`/`.so`) 放置在 `lib/` 目录中。

---

## 开源协议

本软件遵循 [GNU Affero General Public License version 3 with Commons Clause (AGPLv3 + CC)](LICENSE) 开源协议。
