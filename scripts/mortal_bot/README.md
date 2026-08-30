# Mortal external bot (4p + 3p)

This directory contains the **template** that `scripts/setup-mortal-bots.sh`
copies into `mjai_bot/mortal/` (4-player) and `mjai_bot/mortal3p/` (3-player).
The actual bot directories are git-ignored by the repository (`mjai_bot/mortal*`)
on purpose: Mortal is AGPL-3.0 and Akagi intentionally keeps it as a separate
subprocess with no in-process linkage.

Files:

| File                  | Purpose |
| --------------------- | ------- |
| `bot.py`              | `bot.py` for both modes. Loads the external Mortal source + `libriichi[3p]`, speaks Akagi's JSONL bot protocol, and emits a `meta.show` HUD card. |
| `manifest.4p.toml`    | Akagi bot manifest for the 4-player bot. |
| `manifest.3p.toml`    | Akagi bot manifest for the 3-player bot. |
| `pyproject.toml`      | `uv sync` deps (`torch`, `numpy`, `requests`). |

## Why a wrapper instead of vendored code

The Mortal source, the compiled `libriichi[3p]` extension and the `.pth` weights
stay **outside** this repository. `bot.py` only:

1. resolves `mortal_root` (default `<bot>/src`) and optional `libriichi_root`,
2. adds them to `sys.path`,
3. imports the Mortal `model` module from there,
4. either calls the drop's `model.load_model(player_id)` (release-style) or
   builds `Brain`/`DQN`/`MortalEngine`/`mjai.Bot` itself (git-checkout layout),
5. runs the standard mjai stdin/stdout loop.

That preserves Akagi's license boundary and keeps the repo free of AGPL code
and large weight files.

## Two supported layouts

### git checkout (Equim-chan/Mortal, 4p)

```text
mjai_bot/mortal/
├── bot.py
├── pyproject.toml
├── manifest.toml
├── mortal_4p.pth
└── src/                      # a clone of Equim-chan/Mortal
    ├── mortal/
    │   ├── model.py
    │   └── engine.py
    └── libriichi.so|.pyd     # built from src/libriichi
```

`bot.py` imports `model` + `engine` from `<src>/mortal`, builds the engine from
the checkpoint's embedded `config`, and wraps it in `libriichi.mjai.Bot`.

### release-style drop (shinkuan/Akagi-MjaiBot-Mortal, 4p and 3p)

```text
mjai_bot/mortal3p/
├── bot.py
├── pyproject.toml
├── manifest.toml
├── mortal_3p.pth
└── src/
    ├── model.py              # self-contained: Brain/DQN/MortalEngine + load_model()
    ├── libriichi3p.pyd|.so   # or libriichi3p/ per-ABI files + a loader
    └── libriichi/            # the prebuilt 3p files (release3p.zip convention)
```

`bot.py` detects `model.load_model()` on the module and delegates to it; the
drop's own `model.py` builds the engine and returns a `libriichi[3p].mjai.Bot`.
Release drops hard-code `mortal.pth` next to `model.py`; `bot.py` creates a
sibling `mortal.pth` → your `model_file` symlink so you can keep using
`mortal_4p.pth` / `mortal_3p.pth`.

## What the model expects

* **4p**: checkpoint with `config.control.version = 4`, `config.resnet.conv_channels = 192`,
  `num_blocks = 40` (the `mortal_4p.pth` / `mortal.pth` described in the
  request). Needs the **upstream** `libriichi` (`ACTION_SPACE` 46, obs 1012×34).
* **3p**: checkpoint with a smaller Brain (`conv_channels = 32`, `num_blocks = 2`),
  obs 775×**34**, **44 actions** (`mortal_3p.pth`). Needs a **3p-capable**
  `libriichi3p` (upstream Mortal is 4-player only). Obtain the 3p support
  package from the Akagi/Discord release (`bot_3p_0.1.1.zip`) and point
  `libriichi_root` at the directory that contains `libriichi3p.so`/`.pyd`.

The 3p engine keeps the historical 4-shaped mjai grammar (dummy seat 3,
`nukidora` instead of `kita`). `bot.py` translates Akagi's native sanma events
(length-3 `scores`/`tehais`/`deltas`, `kita`) to that shape in, and `nukidora`
→ `kita` on the way out.

## Manual setup (if you don't use the script)

```sh
# 1. Create the bot dirs.
mkdir -p mjai_bot/mortal mjai_bot/mortal3p

# 2. Copy the templates.
cp scripts/mortal_bot/bot.py mjai_bot/mortal/bot.py
cp scripts/mortal_bot/pyproject.toml mjai_bot/mortal/pyproject.toml
cp scripts/mortal_bot/manifest.4p.toml mjai_bot/mortal/manifest.toml
cp scripts/mortal_bot/bot.py mjai_bot/mortal3p/bot.py
cp scripts/mortal_bot/pyproject.toml mjai_bot/mortal3p/pyproject.toml
cp scripts/mortal_bot/manifest.3p.toml mjai_bot/mortal3p/manifest.toml

# 3. Put the Mortal source + built libriichi in place.
#    4p, git checkout:
git clone https://github.com/Equim-chan/Mortal.git mjai_bot/mortal/src
cd mjai_bot/mortal/src/libriichi && cargo build --release
cp target/release/libriichi.so ../libriichi.so    # macOS: libriichi.dylib; Windows: libriichi.pyd (or .dll)
cp /path/to/mortal_4p.pth ../../mortal_4p.pth     # put the 4p weight next to bot.py

#    3p: place the release-style 3p files under mjai_bot/mortal3p/src
#    (model.py with load_model() + libriichi3p.so/.pyd), and mortal_3p.pth
#    next to mjai_bot/mortal3p/bot.py.

# 4. Activate the bots (Settings -> Bots, or edit config.toml):
#    [bot]
#    active_4p = "mortal"
#    active_3p = "mortal3p"
```

## Notes

* The reaction already carries Mortal's raw `meta` (`q_values`, `mask_bits`,
  `shanten`, `at_furiten`, `eval_time_ns`). Akagi's autoplay delay controller
  softmaxes the raw `q_values` itself.
* `bot.py` additionally emits a `meta.show` HUD card (chosen move + top-N
  Q-values). The 4p/3p action raster is a single 64-bit mask: bits 0..33 are
  tiles, 34..36 red fives, 37..45 calls; the 3p fork just leaves the chi/unused
  bits unset, so one label table serves both modes.
* 3p event translation is always on for the `mortal3p` directory; for a custom
  dir set `mode = "3p"` in the settings.
* First `uv sync` pulls `torch` (large). For a CPU-only install on Linux you can
  point `uv` at the CPU index, e.g.
  `UV_INDEX_URL=https://download.pytorch.org/whl/cpu uv sync`.
