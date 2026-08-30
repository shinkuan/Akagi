#!/usr/bin/env bash
#
# Set up Mortal as an Akagi external bot (4-player + 3-player).
#
# Copies the Mortal bot template into mjai_bot/mortal/ and mjai_bot/mortal3p/,
# optionally clones/builds the 4p Mortal checkout + libriichi, and points
# [bot] active_4p / active_3p at them.
#
# The bot directories are git-ignored (mjai_bot/mortal*) and stay AGPL-3.0
# separate from Akagi, exactly like a released Mortal bot drop.
#
# Usage:
#   scripts/setup-mortal-bots.sh [options]
#
# Options:
#   --no-clone             do not git clone the 4p Mortal repo (use existing src)
#   --no-build             do not build libriichi with cargo (use a prebuilt copy)
#   --no-config            do not patch config.toml
#   --mortal-root <path>   Mortal checkout (default <repo>/mjai_bot/mortal/src)
#   --bot-root <path>      bot root (default <repo>/mjai_bot)
#   --config <path>        config.toml to patch
#   --model-4p <path>      optional mortal_4p.pth to copy into the 4p bot dir
#   --model-3p <path>      optional mortal_3p.pth to copy into the 3p bot dir
#   --4p-only / --3p-only  limit which mode is set up
#   --force                overwrite existing bot.py / manifest.toml / pyproject.toml
#
# 3p Mortal is maintained separately from upstream (upstream is 4-player only).
# This script sets up the 3p directory and documents where to drop the 3p
# Mortal checkout + 3p libriichi + mortal_3p.pth; it cannot obtain those files.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATE="$SCRIPT_DIR/mortal_bot"

DO_CLONE=1
DO_BUILD=1
DO_CONFIG=1
DO_4P=1
DO_3P=1
FORCE=0
MORTAL_ROOT=""
BOT_ROOT="$REPO_ROOT/mjai_bot"
CONFIG_FILE=""
MODEL_4P=""
MODEL_3P=""

info()  { printf '\033[1;34m[mortal]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[mortal]\033[0m %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[mortal]\033[0m %s\n' "$*" >&2; }

die() { err "$*"; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-clone) DO_CLONE=0; shift ;;
    --no-build) DO_BUILD=0; shift ;;
    --no-config) DO_CONFIG=0; shift ;;
    --4p-only) DO_3P=0; shift ;;
    --3p-only) DO_4P=0; shift ;;
    --force) FORCE=1; shift ;;
    --mortal-root) MORTAL_ROOT="$2"; shift 2 ;;
    --bot-root) BOT_ROOT="$2"; shift 2 ;;
    --config) CONFIG_FILE="$2"; shift 2 ;;
    --model-4p) MODEL_4P="$2"; shift 2 ;;
    --model-3p) MODEL_3P="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
done

[[ "$DO_4P" == 0 && "$DO_3P" == 0 ]] && die "nothing to do: both 4p and 3p disabled"

mkdir -p "$BOT_ROOT"

# ---------------------------------------------------------------------------
# 4-player bot
# ---------------------------------------------------------------------------
install_bot() {
  local dir="$1" manifest="$2" mode="$3"
  mkdir -p "$dir"
  if [[ "$FORCE" == 0 && -f "$dir/bot.py" ]]; then
    warn "$dir/bot.py already exists; use --force to overwrite (keeps existing .akagi venv)"
    return 0
  fi
  cp "$TEMPLATE/bot.py" "$dir/bot.py"
  cp "$TEMPLATE/pyproject.toml" "$dir/pyproject.toml"
  cp "$TEMPLATE/$manifest" "$dir/manifest.toml"
  cp "$TEMPLATE/README.md" "$dir/README.md"
  if [[ ! -f "$dir/.gitignore" ]]; then
    cat > "$dir/.gitignore" <<'EOF'
.akagi/
src/
src3p/
libriichi3p/
*.pth
*.so
*.pyd
*.dylib
*.dll
*.exe
__pycache__/
EOF
  fi
  info "installed $mode bot -> $dir"
}

if [[ "$DO_4P" == 1 ]]; then
  MORTAL_4P_DIR="$BOT_ROOT/mortal"
  install_bot "$MORTAL_4P_DIR" "manifest.4p.toml" "4p"

  ROOT="$MORTAL_ROOT"
  if [[ -z "$ROOT" ]]; then
    ROOT="$MORTAL_4P_DIR/src"
  fi

  if [[ ! -d "$ROOT" ]]; then
    if [[ "$DO_CLONE" == 1 ]]; then
      info "cloning Equim-chan/Mortal -> $ROOT"
      git clone --depth 1 https://github.com/Equim-chan/Mortal.git "$ROOT"
    else
      warn "Mortal checkout missing at $ROOT (use --mortal-root or --no-clone)"
    fi
  fi

  if [[ -d "$ROOT" && "$DO_BUILD" == 1 ]]; then
    if command -v cargo >/dev/null 2>&1; then
      info "building libriichi (this can take a few minutes)..."
      (cd "$ROOT/libriichi" && cargo build --release)
      BUILD_OUT="$ROOT/libriichi/target/release"
      LIB_SRC=""
      for cand in \
        "$BUILD_OUT"/libriichi.so "$BUILD_OUT"/libriichi.dylib "$BUILD_OUT"/libriichi.dll \
        "$BUILD_OUT"/riichi.so "$BUILD_OUT"/riichi.dylib "$BUILD_OUT"/riichi.dll
      do
        if [[ -f "$cand" ]]; then LIB_SRC="$cand"; break; fi
      done
      if [[ -n "$LIB_SRC" ]]; then
        case "$(basename "$LIB_SRC")" in
          *.dll) cp "$LIB_SRC" "$ROOT/libriichi.pyd" ;;   # rename .dll -> .pyd for Python
          *)     cp "$LIB_SRC" "$ROOT/$(basename "$LIB_SRC")" ;;
        esac
        info "copied libriichi -> $ROOT/$(basename "$LIB_SRC")"
      else
        warn "could not find the built libriichi extension; check $BUILD_OUT/"
      fi
    else
      warn "cargo not found; build libriichi manually and copy it into $ROOT/"
    fi
  fi

  if [[ -n "$MODEL_4P" ]]; then
    cp "$MODEL_4P" "$MORTAL_4P_DIR/mortal_4p.pth"
    info "copied 4p checkpoint -> $MORTAL_4P_DIR/mortal_4p.pth"
  else
    if [[ -f "$MORTAL_4P_DIR/mortal_4p.pth" ]]; then
      info "4p checkpoint present"
    else
      warn "no 4p checkpoint found; put mortal_4p.pth at $MORTAL_4P_DIR/"
    fi
  fi
fi

# ---------------------------------------------------------------------------
# 3-player bot
# ---------------------------------------------------------------------------
if [[ "$DO_3P" == 1 ]]; then
  MORTAL_3P_DIR="$BOT_ROOT/mortal3p"
  install_bot "$MORTAL_3P_DIR" "manifest.3p.toml" "3p"
  mkdir -p "$MORTAL_3P_DIR/src" "$MORTAL_3P_DIR/libriichi3p"

  if [[ -n "$MODEL_3P" ]]; then
    cp "$MODEL_3P" "$MORTAL_3P_DIR/mortal_3p.pth"
    info "copied 3p checkpoint -> $MORTAL_3P_DIR/mortal_3p.pth"
  elif [[ -f "$MORTAL_3P_DIR/mortal_3p.pth" ]]; then
    info "3p checkpoint present"
  else
    warn "no 3p checkpoint found; put mortal_3p.pth at $MORTAL_3P_DIR/"
  fi

  warn "3p Mortal is a separate build (upstream Mortal is 4p only)."
  warn "Either layout works:"
  warn "  - git-checkout fork: put mortal/ (model.py + engine.py) in $MORTAL_3P_DIR/src"
  warn "    and the 3p libriichi .so/.pyd in $MORTAL_3P_DIR/libriichi3p/"
  warn "  - release-style drop: put model.py (with load_model()) + the 3p"
  warn "    libriichi files directly in $MORTAL_3P_DIR/src/"
  warn "Then install the .pth listed above; bot.py detects which layout it is."
fi

# ---------------------------------------------------------------------------
# Activate the bots in config.toml
# ---------------------------------------------------------------------------
if [[ "$DO_CONFIG" == 1 ]]; then
  if [[ -z "$CONFIG_FILE" ]]; then
    # Best-effort: portable zip layout first, then platform user config dirs.
    if [[ -f "$REPO_ROOT/configs/config.toml" ]]; then
      CONFIG_FILE="$REPO_ROOT/configs/config.toml"
    elif [[ -f "$REPO_ROOT/config.toml" ]]; then
      CONFIG_FILE="$REPO_ROOT/config.toml"
    elif [[ -n "${XDG_CONFIG_HOME:-}" && -f "$XDG_CONFIG_HOME/akagi/config.toml" ]]; then
      CONFIG_FILE="$XDG_CONFIG_HOME/akagi/config.toml"
    elif [[ -f "$HOME/.config/akagi/config.toml" ]]; then
      CONFIG_FILE="$HOME/.config/akagi/config.toml"
    elif [[ "$(uname -s)" == "Darwin" && -f "$HOME/Library/Application Support/akagi/config.toml" ]]; then
      CONFIG_FILE="$HOME/Library/Application Support/akagi/config.toml"
    elif [[ -n "${APPDATA:-}" && -f "$APPDATA/akagi/config.toml" ]]; then
      CONFIG_FILE="$APPDATA/akagi/config.toml"
    fi
  fi
  if [[ -z "$CONFIG_FILE" ]]; then
    warn "no config.toml found; set active_4p=\"mortal\" and active_3p=\"mortal3p\" in Akagi Settings -> Bots"
  else
    python3 - "$CONFIG_FILE" "$DO_4P" "$DO_3P" <<'PY'
import re, sys
path = sys.argv[1]
do4p = sys.argv[2] == "1"
do3p = sys.argv[3] == "1"
try:
    with open(path, encoding="utf-8") as f:
        text = f.read()
except FileNotFoundError:
    text = ""

lines = text.split("\n")

# Locate [bot] section boundaries.
bot_start = None
for i, line in enumerate(lines):
    if line.strip() == "[bot]":
        bot_start = i
        break

def upsert(block, key, value, quote=True):
    found = False
    out = []
    for line in block:
        m = re.match(r"^(\s*" + re.escape(key) + r"\s*=)(.*)$", line)
        if m:
            rhs = f'"{value}"' if quote else value
            out.append(f'{m.group(1)} {rhs}')
            found = True
        else:
            out.append(line)
    if not found:
        rhs = f'"{value}"' if quote else value
        out.append(f'{key} = {rhs}')
    return out

bot_block = []
if bot_start is not None:
    j = bot_start + 1
    while j < len(lines) and not re.match(r"^\[", lines[j]):
        j += 1
    bot_block = lines[bot_start + 1:j]
    while bot_block and bot_block[-1] == "":
        bot_block.pop()
    rest = lines[j:]
else:
    rest = []
    bot_start = len(lines)

if do4p or do3p:
    bot_block = upsert(bot_block, "enabled", "true", quote=False)
    if do4p:
        bot_block = upsert(bot_block, "active_4p", "mortal")
    if do3p:
        bot_block = upsert(bot_block, "active_3p", "mortal3p")

if bot_start is None:
    if not text.endswith("\n"):
        text += "\n"
    text += "[bot]\n" + "\n".join(bot_block) + "\n"
else:
    new_lines = lines[:bot_start] + ["[bot]", *bot_block]
    if rest and rest[0].strip().startswith("["):
        new_lines.append("")
    new_lines += rest
    text = "\n".join(new_lines)

# Collapse triple blank lines (from the previous empty tail), keep trailing newline.
text = re.sub(r"\n{3,}", "\n\n", text).rstrip("\n") + "\n"
with open(path, "w", encoding="utf-8") as f:
    f.write(text)
print(f"patched {path}")
PY
  fi
fi

info "done."
cat <<'EOF'

Next steps:
  1. uv sync the bot deps in the Bots tab (or restart Akagi, which syncs on first spawn).
  2. Verify Settings -> Bots shows "Mortal · 4p" and "Mortal · 3p" with env ready.
  3. If the status bar shows "mortal" it is actively recommending; otherwise click
     the bot's 4p/3p switch to activate it.

For the 3p model you still need the separate 3p Mortal files:
  - mjai_bot/mortal3p/src           (3p Mortal checkout)
  - mjai_bot/mortal3p/libriichi3p/  (3p libriichi .so/.pyd)
  - mjai_bot/mortal3p/mortal_3p.pth
EOF
