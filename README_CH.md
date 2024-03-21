<br/>
<p align="center">
  <img src="https://github.com/shinkuan/RandomStuff/assets/35415788/db94b436-c3d4-4c57-893e-8db2074d2d22" width="50%">
  <h1 align="center">Akagi</h3>

  <p align="center">
「死ねば助かるのに………」- 赤木しげる<br>
<br>
    <br/>
    <br/>
    <a href="https://discord.gg/Z2wjXUK8bN">有問題？到Discord找我</a>
    <br/>
    <br/>
    <a href="https://github.com/shinkuan/MajsoulUnlocker/issues">回報 Bug</a>
    .
    <a href="https://github.com/shinkuan/MajsoulUnlocker/issues">功能請求</a>
  </p>
</p>

# 關於

## "這個項目的目的是為了提供人們一種方便的方式，能夠即時了解他們在遊戲對局中的表現，並從中學習和進步。這個項目僅供教育用途，作者對使用此項目的用戶所採取的任何行動不承擔責任。雀魂官方可能會檢測到異常行為，任何後果如賬號暫停，均與作者無關。"

![image](https://github.com/shinkuan/RandomStuff/assets/35415788/4f9b2e2f-059e-44a8-b11a-5b2ce28cb520)

https://github.com/shinkuan/RandomStuff/assets/35415788/ce1b598d-b1d7-49fe-a175-2fe1bd2bb653

# 使用方法

## Flowchart

![Flow](https://github.com/shinkuan/RandomStuff/assets/35415788/85ece51f-cbc7-4236-91eb-95253dcc0132)

## 教程

### 安裝

[點我到 Youtube 觀看安裝影片](https://youtu.be/V7NMNsZ3Ut8)

在開始前，你需要以下東西:

1. `mortal.pth` [(如果你沒有的話，到 Discord 去下載)](https://discord.gg/Z2wjXUK8bN)
2. (Optional) 推薦使用 Windows Terminal，以獲得預期中的 UI 效果。
3. (Optional) 如果你要使用 Steam 或 Majsoul Plus 之類的，請使用類似 Proxifier 的軟體將連線導向至 MITM

**到[Discord](https://discord.gg/Z2wjXUK8bN)下載我提供的 mortal.pth**

1. 到 #verify 頻道點擊 ✅ 驗證身分.
2. 到 #bot-zip
3. 選一個你喜歡的 bot 下載
4. 解壓縮

### Akagi:

#### Windows

到[Release](https://github.com/shinkuan/Akagi/releases/latest)下載`install_akagi.ps1`

1. 將 `install_akagi.ps1` 放在您想安裝 Akagi 的位置。
2. 以**管理員**身份打開 **Powershell**
3. cd 到該目錄
4. 執行： `Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass`
5. 執行： `install_akagi.ps1`
6. 如果這是您第一次使用 mitmproxy，請打開它。
7. 關閉它。
8. 到使用者主目錄 `~/.mitmproxy`
9. 安裝證書。
10. 將 `mortal.pth` 放入 `./Akagi/mjai/bot`

#### Mac

到[Release](https://github.com/shinkuan/Akagi/releases/latest)下載`install_akagi.command`

1. 将 `install_akagi.command` 放在您想安裝 Akagi 的位置。
2. 从[Python 官方网站](https://www.python.org/downloads/)下载最新的 Python 安装包并安装（如已安装其他兼容版本的 Python 可以跳过这一步）。
3. 打开终端，cd到`install_akagi.command`所在的目录。
4. 执行：bash install_akagi.command
5. 安装完成之后，进入Akagi文件夹。
6. 双击run_agaki.command启动agaki。
7. 如果您是第一次使用mitmproxy，请点击start mitm。
5. 关闭它。
6. 到使用者主目录 `~/.mitmproxy`
7. 安装证书。
8. 将 `mortal.pth` 放入 `./Akagi/mjai/bot`

### settings.json

- `Unlocker`: 使用 [MajsoulUnlocker](https://github.com/shinkuan/MajsoulUnlocker)
- `v10`: 如果你的客戶端還在 v0.10.x 版本而且你想要使用 MajsoulUnlocker，設為 true
- `Autoplay`: 自動打牌.
- `Helper`: [mahjong-helper](https://github.com/EndlessCheng/mahjong-helper)
- `Autohu`: Auto Ron.
- `Port`:
  - `MITM`: MITM Port, 你應該將雀魂連線導向到這個 Port.
  - `XMLRPC`: The XMLRPC Port.
  - `MJAI`: The port bind to MJAI bot container.
- `Playwright`:
  - `enable`: Enable the playwright
  - `width`: width of the viewport of playwright
  - `height`: height of the viewport of playwright
- The rest are the setting for MajsoulUnlocker.

## 如何使用

### 主畫面

![image](https://github.com/shinkuan/RandomStuff/assets/35415788/6b66a48b-48fe-4e12-b3cc-18b582410f9a)

可以看到這裡有兩個流程，通常上面的是「大廳」的 Websocket 流程，而下面的是「遊戲」的 Websocket 流程，這個會在加入對局後出現。

點擊下方的流程以開始。（這可能需要一些時間，點擊一次並等待，不要多次點擊）

### 遊戲中的畫面

![image](https://github.com/shinkuan/RandomStuff/assets/35415788/17ae8275-4499-4788-a91b-ecafbac33512)

在進入遊戲流程畫面後，應該會看到這些內容。
左上角是我們使用 MITM 捕獲的 LiqiProto 訊息。
LiqiProto 訊息隨後被轉錄為 mjai 格式並發送給機器人。

右上角是 MJAI 訊息，這是我們的機器人發回給我們的訊息，指示我們應該採取的動作。

然後下方是我們的手牌，它是使用 Unicode 字符組成的。

左下角是設置。

右下角是機器人的動作。

# 如何降低被封號的風險

### 很簡單，乖乖玩遊戲不要搞這些有的沒的。

以下是一些你可以採取的措施。

1. 不要使用 Steam 版，因為它可能會監測你電腦上正在執行的程式。改用 web 版。
2. 使用[Majsoul Mod Plus](https://github.com/Avenshy/majsoul_mod_plus)的`safe_code.js`。
3. 不要開 MajsoulUnlocker，因為它會竄改 websocket 數據。
4. 乖乖手打，不要使用 Autoplay
5. 使用貼圖與你的對手交流。
6. 不要完全照著機器人的指示打牌
7. 不要使用 Autoplay 功能掛機 24h 打牌。

### 目前沒有任何辦法保證完全不封號。

# TODO

- [x] 三麻模式
  - 已完成，但尚未決定公布。
- [x] 在應用程式內更改 Setting。
- [x] 自動打牌
  - [ ] 自動使用貼圖，讓對手認為我們不是機器人。
  - [ ] 在 settings.json 中添加隨機時間，讓用戶選擇他們想要的時間。
- [ ] 混合多個 AI 的決策，讓我們看起來更像人類，而不是完美的機器人。
- [x] 縮短機器人的啟動時間。（也許在遊戲開始前就啟動？）
- [x] 與[MajsoulUnlocker](https://github.com/shinkuan/MajsoulUnlocker)整合
- [ ] 完全不使用 MITM 進行遊戲，使用圖像識別。
  - [ ] 決定使用哪種模型
  - [ ] 訓練數據生成
  - [ ] 進行訓練
  - [ ] 得分差異識別。
  - [ ] 流局識別。
  - [ ] 實施
- [x] 更簡單的安裝流程

## Need Help!

1. 歡迎任何人提交 PR（Pull Request）。
2. 如果你有使用 MajsoulUnlocker，請告訴我它運行得是否順暢，有沒有關於我們修改訊息的痕跡洩露給 Majsoul 伺服器？
3. 尋找一種穩定且安全的自動打牌方式。
4. 如果遇到任何錯誤，請回報。
5. 如果你的機器人很好用，可以考慮分享你的 bot.zip 檔案。

# Authors

- **Shinkuan** - [Shinkuan](https://github.com/shinkuan/)

## 支持作者

### 斗內是自願的，即使不斗內，這個程式的全部功能也是可用的。 <3

ETH Mainnet: 0x83095C4355E43bDFe9cEf2e439F371900664D41F

Paypal 或其他: 到 Discord 找我

You can find me at [Discord](https://discord.gg/Z2wjXUK8bN).

![image](https://github.com/shinkuan/RandomStuff/assets/35415788/2b500d5c-21cd-422c-9684-50477835fe13)

<!-- 未來計劃：

幫助測試仍在開發中的未發布功能

其他可選擇的bot.zip選項 -->

# See Also

### [MajsoulUnlocker](https://github.com/shinkuan/MajsoulUnlocker)

### [Riichi City Unlocker](https://github.com/shinkuan/RiichiCityUnlocker)

# Special Thanks

[Equim-chan/Mortal](https://github.com/Equim-chan/Mortal)

[Majsoul Mod Plus](https://github.com/Avenshy/majsoul_mod_plus)

[mahjong-helper](https://github.com/EndlessCheng/mahjong-helper)

[MahjongRepository/mahjong_soul_api](https://github.com/MahjongRepository/mahjong_soul_api)

[smly/mjai.app](https://github.com/smly/mjai.app)

# LICENSE

```
“Commons Clause” License Condition v1.0

The Software is provided to you by the Licensor under the License, as defined below, subject to the following condition.

Without limiting other conditions in the License, the grant of rights under the License will not include, and the License does not grant to you, the right to Sell the Software.

For purposes of the foregoing, “Sell” means practicing any or all of the rights granted to you under the License to provide to third parties, for a fee or other consideration (including without limitation fees for hosting or consulting/ support services related to the Software), a product or service whose value derives, entirely or substantially, from the functionality of the Software. Any license notice or attribution required by the License must also include this Commons Clause License Condition notice.

Software: Akagi

License: GNU Affero General Public License version 3 with Commons Clause

Licensor: shinkuan
```
