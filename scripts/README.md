# scripts/

Build / release / protocol-update tooling. Each script is invoked from
the repo root.

## `fetch-runtime.sh`

Downloads `python-build-standalone` and `uv` for a target triple, into
`runtime/python/<triple>/` and `runtime/uv/<triple>/`. Idempotent —
re-running with the same versions is a no-op (`--force` to wipe and
re-fetch).

```sh
scripts/fetch-runtime.sh                         # host triple
scripts/fetch-runtime.sh x86_64-pc-windows-msvc  # cross-target
```

Versions come from env vars (`PYTHON_VERSION`, `PBS_RELEASE`,
`UV_VERSION`) with built-in defaults. CI sets them via the `env:` block
at the top of `.github/workflows/release.yml`.

The `runtime/` tree is gitignored. Each per-triple subtree caches under
the same key in CI (`Cache bundled runtime` step), so a second run on
the same target hits the cache and skips network entirely.

## `package-zip.sh`

Stages a portable zip in `dist/akagi-<version>-<os>-<arch>.zip` from a
prebuilt binary plus the fetched runtime tree.

```sh
scripts/package-zip.sh x86_64-unknown-linux-gnu
```

Prerequisites:

- The binary exists at `target/<triple>/release/akagi[.exe]`. Produce it
  with `cargo tauri build --no-bundle --target <triple>` (or plain
  `cargo build --release --target <triple>` if you already ran the
  frontend build separately).
- `runtime/python/<triple>/` and `runtime/uv/<triple>/` are populated by
  `fetch-runtime.sh`.

Outputs a single zip named `akagi-<version>-<os>-<arch>.zip` containing
a top-level folder of the same name with the binary, `runtime/`,
`LICENSE.txt`, `NOTICE`, and a generated `README.txt` with
platform-specific quick-start notes (Gatekeeper xattr on macOS,
SmartScreen on Windows, WebKit2GTK package names on Linux).

The version is parsed from the first `version = "..."` line of
`Cargo.toml` (the `[package]` table is the first table, so this is
unambiguous).

Symlink preservation matters: `python-build-standalone` ships internal
symlinks (`bin/python3.12 → bin/python`). `cp -RP` and `zip -y` are
used so the zip stays small (~half the size of a flattened copy).

## `fetch_liqi.py`

Polled daily by `.github/workflows/auto-liqi.yml`. Fetches the latest
Mahjong Soul `liqi.json` schema from the game CDN and exposes
`changed=true/false` as a GHA output. The workflow then regenerates
`src/bridge/majsoul/proto/liqi.proto` via `pbjs` and opens a PR on `v3`
when the schema moved.

## CI integration

`.github/workflows/release.yml` ties `fetch-runtime.sh` and
`package-zip.sh` together: fetch → `cargo tauri build --no-bundle` →
package → upload `dist/*.zip`. One zip per target (linux-x64,
macos-arm64, windows-x64).
