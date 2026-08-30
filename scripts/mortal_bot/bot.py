#!/usr/bin/env python3
"""Akagi external-bot wrapper for Mortal (https://github.com/Equim-chan/Mortal).

This script is the ``bot.py`` that Akagi spawns for a Mortal bot directory
(`mjai_bot/mortal/` for 4-player, `mjai_bot/mortal3p/` for 3-player).  It does
NOT contain or vendor the Mortal code: the Mortal checkout (``mortal/model.py``,
``mortal/engine.py``, ...) and the compiled ``libriichi`` extension must be
present on disk and are loaded into the process at runtime.  That keeps the
AGPL-3.0 code inside its own subprocess, as the Akagi bot contract intends.

Contract (see `mjai_bot/README.md`):

* Akagi runs ``python bot.py <player_id>`` once per game, cwd = this directory.
* stdin: one JSON array of mjai events per line (a *batch*).
* stdout: exactly one mjai reaction object per line.
* The reaction may carry ``meta`` (Mortal emits raw ``q_values``/``mask_bits``
  and friends; Akagi also accepts a structured ``meta.show`` card, built here).
* ``end_game`` in a batch ends the game; reply then exit.

Runtime settings come from ``AKAGI_BOT_CONFIG`` (a JSON path written by Akagi
from ``manifest.toml`` + ``settings.toml``); the defaults below match the
bundled ``scripts/mortal_bot/manifest.*.toml``.
"""

from __future__ import annotations

import json
import os
import sys
from typing import Any, Dict, List, Optional, Tuple

# --------------------------------------------------------------------------- #
# Mortal legal-action layout (``libriichi/src/mjai/event.rs`` +
# ``libriichi/src/agent/mortal.rs``).
#
# mask_bits is a 64-bit mask over a *single fixed* action raster shared by the
# 4p and 3p engines (the 3p fork simply leaves the manzu 2m..8m and chi bits
# unset).  Bits 0..33 are the 34 tile faces, 34..36 are the red fives, and
# 37..45 are the non-discard calls.  q_values is the compacted list of values
# for the legal actions (ascending bit order), so label reconstruction is
# mask-driven, never a 27-tile projection.
# --------------------------------------------------------------------------- #

TILES = [
    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
    "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p",
    "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s",
    "E", "S", "W", "N", "P", "F", "C",
]
AKA_TILES = ["5mr", "5pr", "5sr"]

ACTION_LABELS_4P = TILES + AKA_TILES + [
    "reach", "chi_low", "chi_mid", "chi_high", "pon", "kan_select",
    "hora", "ryukyoku", "none",
]

# 3p forks keep bits 0..36 (tiles + akadora) and relabel the call slice:
# there is no chi, and a "nukidora" action replaces one of the spare slots.
ACTION_LABELS_3P = TILES + AKA_TILES + [
    "reach", "pon", "kan_select", "nukidora", "hora", "ryukyoku", "none",
    "none", "none",
]

NOTIFY_PREFIX = "@@AKAGI_NOTIFY@@ "


def eprint(*a: Any) -> None:
    print(*a, file=sys.stderr, flush=True)


def notify(level: str, title: str, body: str = "", *, nid: str = "") -> None:
    """Forward a one-line toast to the Akagi frontend (stderr protocol).

    ``nid`` gives the toast a stable id so repeated notifications (e.g. the
    ready toast on each game) replace each other instead of stacking.
    """
    try:
        payload: Dict[str, Any] = {"level": level, "title": title}
        if body:
            payload["body"] = body
        if nid:
            payload["id"] = nid
        print(NOTIFY_PREFIX + json.dumps(payload, ensure_ascii=False), file=sys.stderr, flush=True)
    except Exception:  # pragma: no cover - best-effort
        pass


def resolve_path(value: str, base: str) -> str:
    """Expand env/user vars and make relative paths relative to ``base``."""
    value = os.path.expandvars(os.path.expanduser(value.strip()))
    if not value:
        return ""
    if os.path.isabs(value):
        return value
    return os.path.join(base, value)


def load_settings() -> Dict[str, Any]:
    path = os.environ.get("AKAGI_BOT_CONFIG", "")
    if not path or not os.path.isfile(path):
        return {}
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        return data if isinstance(data, dict) else {}
    except Exception as ex:  # pragma: no cover - defensive
        eprint(f"warning: could not read AKAGI_BOT_CONFIG {path}: {ex}")
        return {}


def get_bool(settings: Dict[str, Any], key: str, default: bool) -> bool:
    v = settings.get(key, default)
    if isinstance(v, bool):
        return v
    if isinstance(v, str):
        return v.strip().lower() in ("1", "true", "yes", "on")
    return default


def get_float(settings: Dict[str, Any], key: str, default: float, lo: float, hi: float) -> float:
    try:
        v = float(settings.get(key, default))
    except (TypeError, ValueError):
        v = default
    return min(hi, max(lo, v))


def get_player_id() -> int:
    env = os.environ.get("AKAGI_PLAYER_ID", "")
    if env:
        try:
            return int(env)
        except ValueError:
            pass
    if len(sys.argv) > 1:
        try:
            return int(sys.argv[-1])
        except ValueError:
            pass
    return 0


def add_import_paths(settings: Dict[str, Any], bot_dir: str, mode: str) -> Tuple[str, str, str]:
    """Return ``(mortal_root, model_dir, lib_pkg)`` and wire up ``sys.path``.

    The Mortal fork used for sanma exposes the native extension under a
    different top-level package name (``libriichi3p``), so the caller must
    import the right one; we only make sure the matching directory is on
    ``sys.path`` and let `model.py`/`engine.py` import that package
    themselves.
    """
    mortal_root = resolve_path(str(settings.get("mortal_root") or "src"), bot_dir)
    lib_root_raw = str(settings.get("libriichi_root") or "")
    libriichi_root = (
        resolve_path(lib_root_raw, bot_dir) if lib_root_raw.strip() else mortal_root
    )
    # model.py / engine.py live one directory under a git checkout
    # (`src/mortal/`); a release-style drop may put them at the root too.
    model_dir = os.path.join(mortal_root, "mortal")
    if not os.path.isfile(os.path.join(model_dir, "model.py")) and os.path.isfile(
        os.path.join(mortal_root, "model.py")
    ):
        model_dir = mortal_root

    paths = []
    if libriichi_root and os.path.isdir(libriichi_root):
        paths.append(libriichi_root)
    if os.path.isdir(mortal_root):
        paths.append(mortal_root)
    if model_dir != mortal_root and os.path.isdir(model_dir):
        paths.append(model_dir)

    for p in paths:
        if p not in sys.path:
            sys.path.insert(0, p)

    lib_pkg = "libriichi3p" if mode == "3p" else "libriichi"
    return mortal_root, model_dir, lib_pkg


def load_checkpoint(model_file: str):
    """Load the Mortal checkpoint, tolerating older ``weights_only`` files."""
    import torch  # noqa: E402  (installed by pyproject)

    try:
        return torch.load(model_file, weights_only=True, map_location="cpu")
    except Exception:
        # Older checkpoints (or a 3p fork) may contain objects `weights_only`
        # refuses. This is a locally-trusted model file, so fall back with a
        # warning rather than failing to start.
        eprint("warning: torch.load(weights_only=True) failed; retrying with weights_only=False")
        return torch.load(model_file, weights_only=False, map_location="cpu")


def build_engine(settings: Dict[str, Any], bot_dir: str, player_id: int, mode: str):
    """Load brain/dqn and build the Mortal engine + mjai Bot."""
    import torch  # noqa: E402  (installed by pyproject)

    model_file = resolve_path(str(settings.get("model_file") or "mortal_4p.pth"), bot_dir)
    if not os.path.isfile(model_file):
        raise FileNotFoundError(
            f"model file not found: {model_file}\n"
            "Put the Mortal checkpoint there (mortal_4p.pth / mortal_3p.pth) "
            "or point `model_file` at it in bot settings."
        )

    mortal_root, model_dir, _lib_pkg = add_import_paths(settings, bot_dir, mode)
    if not os.path.isdir(model_dir):
        raise FileNotFoundError(
            f"Mortal checkout not found at {model_dir}\n"
            "Clone/copy the Mortal repo there (or set `mortal_root`); "
            "see scripts/mortal_bot/README.md."
        )

    # Import only after sys.path is ready. `libriichi[3p]` must be importable
    # before `model` because model.py reads libriichi.consts.
    try:
        import model as model_mod  # noqa: E402
    except ImportError as ex:
        missing = getattr(ex, "name", None) or ""
        if missing in ("libriichi", "libriichi3p"):
            raise RuntimeError(
                f"could not import Mortal modules (mortal_root={mortal_root}): {ex}\n"
                "Build/copy `libriichi.so`/`.pyd` into the Mortal root or "
                "`libriichi_root` first."
            ) from ex
        raise RuntimeError(
            f"could not import Mortal `model` (mortal_root={mortal_root}): {ex}\n"
            f"Missing Python dependency: {missing or ex}. "
            "Run `uv sync` in the Mortal bot directory (or reinstall it from "
            "Settings -> Bots) so the bot venv matches pyproject.toml."
        ) from ex

    # A release-style Mortal drop (e.g. shinkuan/Akagi-MjaiBot-Mortal) is
    # self-contained: `model.py` already builds Brain/DQN/MortalEngine and
    # exposes `load_model(seat)` returning a `libriichi[3p].mjai.Bot`.  Those
    # drops consume a *batch* (JSON array) on `react()` and do their own
    # start_game/end_game handling, so the main loop marks them `batch_style`.
    # They also hard-code `mortal.pth` next to model.py; if the user's
    # checkpoint is named differently and lives elsewhere, expose it as a
    # sibling symlink so `load_model` finds it without editing model.py.
    if callable(getattr(model_mod, "load_model", None)):
        mdir = os.path.dirname(os.path.abspath(model_mod.__file__))
        canonical = os.path.join(mdir, "mortal.pth")
        if not os.path.isfile(canonical) and os.path.isfile(model_file):
            if os.path.abspath(model_file) != os.path.abspath(canonical):
                try:
                    os.symlink(model_file, canonical)
                except OSError:
                    pass
        return model_mod.load_model(player_id), None, True

    state = load_checkpoint(model_file)
    try:
        import engine as engine_mod  # noqa: E402
    except ImportError as ex:
        raise RuntimeError(
            f"could not import Mortal `engine` (mortal_root={mortal_root}): {ex}\n"
            "A git checkout has `mortal/model.py` + `mortal/engine.py`; a "
            "release drop has a self-contained `model.py` with `load_model()`."
        ) from ex

    cfg = state.get("config", {})
    control = cfg.get("control", {})
    resnet = cfg.get("resnet", {})
    version = int(control.get("version", 1))
    conv_channels = int(resnet.get("conv_channels", 192))
    num_blocks = int(resnet.get("num_blocks", 2))

    brain = model_mod.Brain(
        version=version,
        num_blocks=num_blocks,
        conv_channels=conv_channels,
    ).eval()
    dqn = model_mod.DQN(version=version).eval()
    brain.load_state_dict(state["mortal"])
    dqn.load_state_dict(state["current_dqn"])

    device_name = str(settings.get("device") or "cpu")
    device = torch.device(device_name)

    engine = engine_mod.MortalEngine(
        brain,
        dqn,
        version=version,
        is_oracle=False,
        device=device,
        enable_amp=get_bool(settings, "enable_amp", False),
        enable_quick_eval=get_bool(settings, "enable_quick_eval", True),
        enable_rule_based_agari_guard=get_bool(
            settings, "enable_rule_based_agari_guard", True
        ),
        name=str(settings.get("name") or "mortal"),
        boltzmann_epsilon=get_float(settings, "boltzmann_epsilon", 0.0, 0.0, 1.0),
        boltzmann_temp=get_float(settings, "boltzmann_temp", 1.0, 0.01, 100.0),
        top_p=get_float(settings, "top_p", 1.0, 0.0, 1.0),
    )

    try:
        from libriichi.mjai import Bot as MjaiBot  # noqa: E402
    except ImportError:
        from libriichi3p.mjai import Bot as MjaiBot  # noqa: E402
    return MjaiBot(engine, player_id), state, False


# --------------------------------------------------------------------------- #
# `meta.show` card (Akagi's structured HUD payload). Best-effort; the raw
# `q_values`/`mask_bits` Mortal already emits stay in `meta` for the autoplay
# delay model (which softmaxes raw Q values itself).
# --------------------------------------------------------------------------- #

def action_label(idx: int, mode: str) -> Tuple[str, List[str]]:
    labels = ACTION_LABELS_3P if mode == "3p" else ACTION_LABELS_4P
    if 0 <= idx < len(labels):
        label = labels[idx]
        if 0 <= idx < len(TILES):
            return ("Discard", [TILES[idx]])
        if len(TILES) <= idx < len(TILES) + len(AKA_TILES):
            return ("Discard", [AKA_TILES[idx - len(TILES)]])
        return {
            "reach": ("Riichi", []),
            "chi_low": ("Chi (low)", []),
            "chi_mid": ("Chi (mid)", []),
            "chi_high": ("Chi (high)", []),
            "pon": ("Pon", []),
            "kan_select": ("Kan", []),
            "nukidora": ("Kita", ["N"]) if mode == "3p" else ("Nukidora", ["N"]),
            "hora": ("Hora", []),
            "ryukyoku": ("Ryukyoku", []),
            "none": ("Skip", []),
        }.get(label, (f"Action {idx}", []))
    return (f"Action {idx}", [])


def reaction_label(ev: Dict[str, Any]) -> Optional[Tuple[str, List[str]]]:
    typ = ev.get("type")
    pai = ev.get("pai")
    consumed = ev.get("consumed") or []
    if typ == "dahai" and pai:
        return ("Discard", [pai])
    if typ == "reach":
        return ("Riichi", [pai] if pai else [])
    if typ == "pon":
        return ("Pon", [pai] + list(consumed))
    if typ == "chi":
        return ("Chi", [pai] + list(consumed))
    if typ == "daiminkan":
        return ("Kan", [pai] + list(consumed))
    if typ == "ankan":
        return ("Ankan", list(consumed))
    if typ == "kakan":
        return ("Kakan", [pai] + list(consumed))
    if typ == "hora":
        return ("Hora", [])
    if typ == "ryukyoku":
        return ("Ryukyoku", [])
    if typ == "kita":
        return ("Kita", [pai] if pai else ["N"])
    return None


def build_show(
    ev: Dict[str, Any],
    meta: Dict[str, Any],
    mode: str,
    top_n: int,
    include_candidates: bool,
) -> Optional[Dict[str, Any]]:
    items: List[Dict[str, Any]] = []
    first = reaction_label(ev)
    if first is not None:
        item: Dict[str, Any] = {"label": first[0]}
        if first[1]:
            item["pais"] = first[1]
        items.append(item)

    # Mortal reports q_values only for *legal* actions, compacted in ascending
    # action-index order; mask_bits tells us the original indices.  Reconstruct
    # them so the HUD can rank the alternatives.
    q_vals = meta.get("q_values") if include_candidates else None
    mask_bits = meta.get("mask_bits")
    if isinstance(q_vals, list) and q_vals:
        indexed = None
        if isinstance(mask_bits, int):
            legal = [i for i in range(64) if mask_bits & (1 << i)]
            indexed = list(zip(legal, q_vals))
        if indexed is None or len(indexed) != len(q_vals):
            # Defensive: if mask reconstruction doesn't line up, rank by order.
            indexed = list(enumerate(q_vals))
        ranked = sorted(indexed, key=lambda t: t[1], reverse=True)
        seen: set[int] = set()
        for idx, q in ranked:
            if idx in seen:
                continue
            seen.add(idx)
            label, pais = action_label(int(idx), mode)
            item = {"label": label, "value": f"{float(q):.3f}"}
            if pais:
                item["pais"] = pais
            items.append(item)
            if len(items) >= top_n + 1:
                break

    if len(items) <= 1:
        return None
    return {"title": f"Mortal · {mode.upper()}", "items": items}


def libriichi3p_inbound(ev: Dict[str, Any]) -> Dict[str, Any]:
    """Translate Akagi's native sanma mjai into the libriichi3p shape.

    libriichi3p (the Mortal-3p fork) retains the *historical* 4-shaped mjai
    grammar: 3-player games are represented with a dummy seat 3, and the
    BaBei-sets-aside event is spelled ``nukidora`` rather than ``kita``.
    Akagi's schema emits native sanma (length-3 arrays, ``kita``), so pad /
    rename before handing the event to the engine.  For 4p this is a no-op.
    """
    out = dict(ev)

    if out.get("type") == "start_kyoku":
        if isinstance(out.get("scores"), list) and len(out["scores"]) == 3:
            out["scores"] = out["scores"] + [0]
        if isinstance(out.get("tehais"), list) and len(out["tehais"]) == 3:
            out["tehais"] = out["tehais"] + [["?"] * 13]

    if out.get("type") == "kita":
        out = {**out, "type": "nukidora", "pai": "N"}

    if isinstance(out.get("deltas"), list) and len(out["deltas"]) == 3:
        out["deltas"] = out["deltas"] + [0]

    return out


def libriichi3p_outbound(ev: Dict[str, Any]) -> Dict[str, Any]:
    """Reverse ``libriichi3p_inbound`` on an engine reaction."""
    if ev.get("type") != "nukidora":
        return ev
    out = {"type": "kita", "actor": ev.get("actor")}
    if ev.get("meta") is not None:
        out["meta"] = ev["meta"]
    return out


def attach_show(reaction: str, mode: str, settings: Dict[str, Any]) -> str:
    if not reaction:
        return reaction
    try:
        obj = json.loads(reaction)
    except (ValueError, TypeError):
        return reaction
    if not isinstance(obj, dict) or obj.get("type") == "none":
        return reaction

    inject = get_bool(settings, "emit_show", True)
    if not inject:
        return reaction
    meta = obj.get("meta") or {}
    top_n = int(get_float(settings, "show_top_n", 3, 1, 10))
    include_candidates = get_bool(settings, "emit_show_candidates", True)
    show = build_show(obj, meta, mode, top_n, include_candidates)
    if show is not None:
        meta["show"] = show
        obj["meta"] = meta
        return json.dumps(obj, ensure_ascii=False, separators=(",", ":"))
    return json.dumps(obj, ensure_ascii=False, separators=(",", ":"))


def strip_show(reaction: str) -> str:
    """Remove a pre-existing ``meta.show`` card (used when emit_show=false)."""
    try:
        obj = json.loads(reaction)
    except (ValueError, TypeError):
        return reaction
    if not isinstance(obj, dict):
        return reaction
    meta = obj.get("meta")
    if isinstance(meta, dict) and "show" in meta:
        del meta["show"]
        if meta:
            obj["meta"] = meta
        else:
            obj.pop("meta", None)
    return json.dumps(obj, ensure_ascii=False, separators=(",", ":"))


def feed_batch_style(
    bot: Any,
    batch: List[Dict[str, Any]],
    settings: Dict[str, Any],
    mode: str,
) -> str:
    """Drive a release-style drop whose ``react()`` consumes one JSON batch.

    Such drops already handle native sanma→libriichi3p conversion, reach
    speculation and their own ``meta.show``, so this wrapper only applies the
    HUD on/off preference without re-translating events.
    """
    try:
        reaction = bot.react(
            json.dumps(batch, ensure_ascii=False, separators=(",", ":")),
        )
    except Exception as ex:
        eprint(f"Mortal react failed: {ex}")
        notify("error", "Mortal react error", str(ex))
        reaction = None

    out = reaction or '{"type":"none"}'
    if not get_bool(settings, "emit_show", True):
        return strip_show(out)
    # Keep the drop's own richer show; only add ours as a fallback when it
    # didn't produce one.
    try:
        if "meta" in json.loads(out) and "show" in json.loads(out)["meta"]:
            return out
    except (ValueError, TypeError):
        pass
    return attach_show(out, mode, settings)


def main() -> None:
    bot_dir = os.path.dirname(os.path.abspath(__file__))
    settings = load_settings()
    mode = str(settings.get("mode") or ("3p" if "3p" in os.path.basename(bot_dir) else "4p"))
    is_3p = mode == "3p"
    player_id = get_player_id()

    try:
        bot, _state, batch_style = build_engine(settings, bot_dir, player_id, mode)
    except Exception as ex:
        eprint(f"failed to init Mortal: {ex}")
        notify("error", "Mortal failed to load", str(ex))
        sys.exit(1)

    if player_id not in range(4):
        eprint(f"player_id must be in [0,3], got {player_id}")
        notify("error", "Mortal invalid seat", f"player_id={player_id}")
        sys.exit(1)

    notify("info", "Mortal ready", f"seat {player_id} ({mode})", nid="mortal-ready")

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            batch = json.loads(line)
        except ValueError:
            eprint(f"skipping malformed batch: {line[:120]}")
            continue
        if not isinstance(batch, list):
            batch = [batch]

        saw_end = any(
            isinstance(ev, dict) and ev.get("type") == "end_game" for ev in batch
        )

        if batch_style:
            out = feed_batch_style(bot, batch, settings, mode)
            print(out, flush=True)
            if saw_end:
                break
            continue

        reaction: Optional[str] = None
        for raw_ev in batch:
            if not isinstance(raw_ev, dict):
                continue
            ev = libriichi3p_inbound(raw_ev) if is_3p else raw_ev
            try:
                reaction = bot.react(
                    json.dumps(ev, ensure_ascii=False, separators=(",", ":")),
                )
            except Exception as ex:
                eprint(f"Mortal react failed: {ex}")
                notify("error", "Mortal react error", str(ex))
                reaction = None
                break

        out = reaction or '{"type":"none"}'
        if is_3p:
            try:
                obj = json.loads(out)
                if isinstance(obj, dict):
                    obj = libriichi3p_outbound(obj)
                    out = json.dumps(obj, ensure_ascii=False, separators=(",", ":"))
            except (ValueError, TypeError):
                pass
        out = attach_show(out, mode, settings)
        print(out, flush=True)

        if saw_end:
            break


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:  # pragma: no cover
        pass
