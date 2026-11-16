# Automatic continuation for Mahjong Soul (Windows prototype)

Adds an opt-in **Automatic continuation** card at the top of Settings. The quota counts extra matches, excluding the current match. A quota of 3 starts three additional matches, then disables continuation. Users can disable the quota for continuous play. Every application launch starts with continuation off.

The verified flow is:

```text
complete match → terminal ranking/points confirmations
→ rematch button → full rematch confirmation prompt
→ new authenticated match → decrement quota
```

Reconnections and duplicate end events do not consume the quota. Stopping does not leave the current match or cancel an already submitted queue. Connection errors, manual matchmaking, tab changes and timeouts stop continuation. Terminal confirmation requires both a title and button template, stable repeated detection, and is limited to three clicks with a delay. Rematch confirmation requires the complete dialog prompt; a button template covers OCR misses of the artistic label.

## Architecture

`NextGameCard` calls Tauri commands. `NextGameService` owns a hidden Node worker with a random loopback port and token; the token never enters the webview. The worker reads Akagi's logs and controls only the Chromium game tab launched by that installation. Closing Akagi terminates the worker; the worker also checks its parent process. Screenshots and OCR stay local.

## Build and package

Build the frontend and Akagi using the repository's normal build instructions. Then run this from the repository root, using a trusted Node.js 24 Windows x64 executable:

```powershell
.\scripts\package-next-game.ps1 -Destination 'C:\path\to\portable-akagi' -NodeExe 'C:\path\to\node.exe'
```

The resulting `next-game` directory must sit beside `akagi.exe`. Node is not committed to this repository. Windows PowerShell 5.1 and Windows OCR with Chinese language support are required. Enable Mahjong Soul, Chromium capture and autoplay in Akagi before enabling continuation.

## Validation

```powershell
node --test next-game/core.test.mjs next-game/log-reader.test.mjs next-game/embedded.test.mjs
powershell.exe -NoProfile -ExecutionPolicy Bypass -File next-game/button-match.test.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File next-game/result-match.test.ps1
# In frontend:
npm run test -- src/components/NextGameCard.test.tsx
npm run build
```

Live Windows testing covered terminal ranking/points, rematch confirmation, a new authenticated match, and quota-one exhaustion (`started=1`, `remaining=0`, `enabled=false`). Native Settings IPC and autoplay recovery after application restart were verified.

## Draft scope and remaining integration

This is a Windows prototype verified against the Traditional Chinese Mahjong Soul Unity web client. Other languages, platforms, game layouts and game modes are unverified. The UI and worker status strings currently use Simplified Chinese and need integration with the application's localization system. The upstream release workflows do not yet package the Node worker. Cross-platform support, packaging/runtime distribution, and the longer-term choice between a worker and a native implementation need maintainer review before merging.

The four PNG files are small UI recognition crops from Mahjong Soul's game interface (terminal title and action buttons); they contain no player names or account data. They are matching fixtures, not original Akagi artwork. Full game screenshots, logs, local configuration and runtime executables are not part of this contribution.
