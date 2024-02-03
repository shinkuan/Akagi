<br/>
<p align="center">
  <img src="https://github.com/shinkuan/RandomStuff/assets/35415788/db94b436-c3d4-4c57-893e-8db2074d2d22" width="50%">
  <h1 align="center">Akagi</h3>

  <p align="center">
「死ねば助かるのに………」- 赤木しげる<br>
<br>
    <br/>
    <br/>
    <a href="https://discord.gg/Z2wjXUK8bN">Ask me anything about this at Discord</a>
    <br/>
    <br/>
    <a href="https://github.com/shinkuan/Akagi/blob/main/README_CH.md">中文</a>
    <br/>
    <a href="https://github.com/shinkuan/MajsoulUnlocker/issues">Report Bug</a>
    .
    <a href="https://github.com/shinkuan/MajsoulUnlocker/issues">Request Feature</a>
  </p>
</p>

# About The Project

## "The purpose of this project is to provide people with a convenient way to real-time understand their performance in Majsoul game matches and to learn and improve from it. This project is intended for educational purposes only, and the author is not responsible for any actions taken by users using this project. Majsoul officials may detect abnormal behavior, and any consequences such as account suspension are not related to the author."

![image](https://github.com/shinkuan/RandomStuff/assets/35415788/4f9b2e2f-059e-44a8-b11a-5b2ce28cb520)

https://github.com/shinkuan/RandomStuff/assets/35415788/ce1b598d-b1d7-49fe-a175-2fe1bd2bb653

# Usage

## Flowchart

![Flow](https://github.com/shinkuan/RandomStuff/assets/35415788/85ece51f-cbc7-4236-91eb-95253dcc0132)

## Setup

### Installation.

[YouTube Video for you to follow.](https://youtu.be/70m67GezilY)

### You will need:
1. A `mortal.pth`. [(Get one from Discord server if you don't have one.)](https://discord.gg/Z2wjXUK8bN)
2. A `libriichi` that match your system. [(Get it here)](https://github.com/shinkuan/Akagi/releases/tag/v0.1.0-libriichi)
3. (Optional, Recommend) Use Windows Terminal to open client.py for a nice looking TUI.
4. (Optional) If you want to use Steam, Majsoul Plus, or anything other client, proxy the client using tools like proxifier.

__Get mortal.pth at [Discord](https://discord.gg/Z2wjXUK8bN)__
1. Go to #verify and click the ✅ reaction.
2. Go to #bot-zip
3. Download a bot you like.
4. Extract the zip.
5. And mortal.pth is there.

If you are on MacOSX or Linux, try [libriichi builds](https://github.com/shinkuan/Akagi/blob/main/mjai/bot/libriichi_builds) for your platform.

### Akagi:

Download `install_akagi.ps1` at [Release](https://github.com/shinkuan/Akagi/releases/latest)

1. Put `install_akagi.ps1` at the location you want to install Akagi.
2. Open **Powershell** as **Administrator**
3. cd in to the directory
4. Run: `Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass`
5. Run: `install_akagi.ps1`
6. Open mitmproxy if this is your first time using it.
7. Close it.
8. Go to your user home directory `~/.mitmproxy`
9. Install the certificate.
10. Put `libriichi` into `./Akagi/mjai/bot` and rename it as `libriichi`
11. Put `mortal.pth` into `./Akagi/mjai/bot`

### settings.json

 - `Unlocker`: Decide to use [MajsoulUnlocker](https://github.com/shinkuan/MajsoulUnlocker) or not.
 - `v10`: If your Majsoul client in still in v0.10.x and you want to use MajsoulUnlocker, set it to true.
 - `Autoplay`: Autoplay.
 - `Helper`: To use [mahjong-helper](https://github.com/EndlessCheng/mahjong-helper) or not
 - `Autohu`: Auto Ron.
 - `Port`:
   - `MITM`: The MITM Port, you should redirect Majsoul connection to this port.
   - `XMLRPC`: The XMLRPC Port.
   - `MJAI`: The port bind to MJAI bot container.
 - `Playwright`:
   - `enable`: Enable the playwright
   - `width`: width of the viewport of playwright
   - `height`: height of the viewport of playwright
 - The rest are the setting for MajsoulUnlocker.

## Instructions

### Main Screen

![image](https://github.com/shinkuan/RandomStuff/assets/35415788/6b66a48b-48fe-4e12-b3cc-18b582410f9a)

You can see that there are two flows here, usually the top one is the "Lobby" websocket flow, and the bottom one is the "Game" websocket flow which appears after you join a match.

Click on the bottom flow to start. (It can take a while, click once and wait, don't click it for multiple times)

### Flow Screen

![image](https://github.com/shinkuan/RandomStuff/assets/35415788/17ae8275-4499-4788-a91b-ecafbac33512)

After you are in the Flow Screen, this is what you should see.
On top left is the LiqiProto Message we captured using MITM.
The LiqiProto Message is then transcribe to mjai format and send to the bot(AI).

On top right is the MJAI Messages, this is the message our bot sent back to us, indicating the action we should do.

Then below is our tehai, it is composed using unicode characters.

Bottom left is the settings. 

Bottom right is the bot's action.

# How to keep your account safe

Following guide can minimum the probility of account suspension.

1. Don't use steam, use web instead.
2. Use `safe_code.js` from [Majsoul Mod Plus](https://github.com/Avenshy/majsoul_mod_plus)
3. Don't use MajsoulUnlocker as it modifies websocket.
4. Don't use autoplay, play it yourself.
5. Try to use stickers often.
6. Don't completely follow what bot told you to do.
7. Don't play 24hr a day using autoplay.

### There is no way to guarantee 100% no account suspension currently.

# TODO
 - [x] 3 Player Mahjong
   - Already done, but not planned to release yet.
 - [x] Change Setting inside application.
 - [x] Autoplay
   - [ ] Auto use stickers to make opponent think we are not a bot.
   - [ ] Add random time in settings.json to let user choose time they want.
 - [ ] Mix multiple AI's decision to make we more like a human but not a perfect bot.
 - [x] Reduce Startup time of the bot. (Maybe start it before match begin?)
 - [x] Intergrade with [MajsoulUnlocker](https://github.com/shinkuan/MajsoulUnlocker)
 - [ ] Don't use MITM at all for the gameplay, use image recognition.
   - [ ] Decide use what model
   - [ ] Training data generation
   - [ ] Train it
   - [ ] Delta Score Recognition.
   - [ ] Ryukyoku Recognition.
   - [ ] Implement
 - [x] Easier installation.

## Need Help!

1. Any PR is welcomed.
2. Tell me if the MajsoulUnlocker is working well, is there any trace about we modified the message leaked to Majsoul server?
3. A stable and safe way to autoplay.
4. Report any bug you encounter.
5. Share your bot.zip if it is good.

# Authors

* **Shinkuan** - [Shinkuan](https://github.com/shinkuan/)

## Support me

### Donating is optional, and the full functionality of this program is avaliable even without a donation.

ETH Mainnet: 0x83095C4355E43bDFe9cEf2e439F371900664D41F

Paypal or Others: Contact me on Discord.

You can find me at [Discord](https://discord.gg/Z2wjXUK8bN).

### What can I get after donating?

Firstly, thank you very much for your willingness to support the author. 

I will prioritize the opinions of donors, such as feature requests and bug fixes.

Next, you can find me on Discord, where I will assign you a donor role.

<!-- Planned in future:

- Help test unreleased features that are still in development
- Other bot.zip options to choose from -->

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