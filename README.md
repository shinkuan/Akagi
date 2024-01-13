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

### Install

- In Dev

You will need:
1. Docker
2. Proxifier or something that can redirect connection to mitmproxy.
3. A `bot.zip` that contains the AI. [Examples](https://github.com/smly/mjai.app/tree/main/examples)
4. (Optional) Use Windows Terminal to open client.py for a nice looking TUI.

Docker:
1. Install Docker on your PC
2. `docker pull smly/mjai-client:v3`

Akagi:
1. `git clone this`
2. `cd into this repo`
3. Create a Python Virtual Env `python -m venv venv`
4. Activate it. `.\venv\Scripts\activate`
5. `pip install -r requirement.txt`
6. `cd mjai.app`
7. Install mjai `pip install -e .`
8. put `bot.zip` into ./player folder

### Run

After you activate the venv:
1. Configure your setting at `setting.json`
2. `python mitm.py`
3. `python client.py`

### settings.json

 - `Unlocker`: Decide to use [MajsoulUnlocker](https://github.com/shinkuan/MajsoulUnlocker) or not.
 - `v10`: If your Majsoul client in still in v0.10.x and you want to use MajsoulUnlocker, set it to true.
 - `Autoplay`: Autoplay.
 - `Port`:
   - `MITM`: The MITM Port, you should redirect Majsoul connection to this port.
   - `XMLRPC`: The XMLRPC Port.
   - `MJAI`: The port bind to MJAI bot container.
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

Bottom left is the settings. (WIP, currently you should change settings via settings.json)

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
 - [ ] Change Setting inside application.
 - [x] Autoplay - (Working now, but it is __not__ stable)
   - [ ] Auto use stickers to make opponent think we are not a bot.
 - [ ] Add random time in settings.json to let user choose time they want.
 - [ ] Mix multiple AI's decision to make we more like a human but not a perfect bot.
 - [ ] Reduce Startup time of the bot. (Maybe start it before match begin?)
 - [x] Intergrade with [MajsoulUnlocker](https://github.com/shinkuan/MajsoulUnlocker)
 - [ ] Don't use MITM at all for the gameplay, use image recognition.
   - [ ] Decide use what model
   - [ ] Training data generation
   - [ ] Train it
   - [ ] Delta Score Recognition.
   - [ ] Ryukyoku Recognition.
   - [ ] Implement
 - [ ] Easier installation.

## Need Help!

1. Any PR is welcomed.
2. Tell me if the MajsoulUnlocker is working well, is there any trace about we modified the message leaked to Majsoul server?
3. A stable and safe way to autoplay.
4. Report any bug you encounter.
5. Share your bot.zip if it is good.

# Authors

* **Shinkuan** - [Shinkuan](https://github.com/shinkuan/)

## Support me

__Donating is optional, and the full functionality of this program is avaliable even without a donation.__

ETH Mainnet: 0x83095C4355E43bDFe9cEf2e439F371900664D41F

Paypal: Maybe? Contact me.

If you've made a donation to support me, feel free to let me know what feature or enhancement you'd like to see in the future!

You can find me at [Discord](https://discord.gg/Z2wjXUK8bN).

# See Also

### [MajsoulUnlocker](https://github.com/shinkuan/MajsoulUnlocker)

### [Riichi City Unlocker](https://github.com/shinkuan/RiichiCityUnlocker)

# Special Thanks

[Majsoul Mod Plus](https://github.com/Avenshy/majsoul_mod_plus)

[MahjongRepository/mahjong_soul_api](https://github.com/MahjongRepository/mahjong_soul_api)

[smly/mjai.app](https://github.com/smly/mjai.app)
