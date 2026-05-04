#!/usr/bin/env bash
#
# Fetch python-build-standalone + uv for a target triple and drop them
# into <repo-root>/runtime/{python,uv}/<triple>/. `scripts/package-zip.sh`
# copies that tree next to the binary in the portable zip; `try_bundled`
# in src/bot/runtime.rs then picks it up exe-adjacent at runtime.
#
# Usage:
#   scripts/fetch-runtime.sh                         # host triple
#   scripts/fetch-runtime.sh x86_64-pc-windows-msvc  # cross-target
#
# Env overrides:
#   PYTHON_VERSION  default: 3.12.13
#   PBS_RELEASE     default: 20260414
#   UV_VERSION      default: 0.11.8
#
# Layout produced:
#   runtime/python/<triple>/        # full python-build-standalone tree
#                          /bin/python3   (linux/mac)
#                          /python.exe    (windows)
#   runtime/uv/<triple>/uv          (linux/mac)
#   runtime/uv/<triple>/uv.exe      (windows)
#
# Re-runs are idempotent: if the python binary and uv binary already
# exist for the requested triple, fetch is skipped. Pass --force to
# wipe and reinstall.

set -euo pipefail

PYTHON_VERSION="${PYTHON_VERSION:-3.12.13}"
PBS_RELEASE="${PBS_RELEASE:-20260414}"
UV_VERSION="${UV_VERSION:-0.11.8}"

FORCE=0
TARGET=""
for arg in "$@"; do
  case "$arg" in
    --force|-f) FORCE=1 ;;
    -h|--help)
      sed -n '2,30p' "$0"
      exit 0
      ;;
    *) TARGET="$arg" ;;
  esac
done

if [[ -z "$TARGET" ]]; then
  if ! command -v rustc >/dev/null 2>&1; then
    echo "no target arg and rustc not on PATH; pass a triple explicitly" >&2
    exit 1
  fi
  TARGET="$(rustc -vV | awk '/host:/ { print $2 }')"
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${ROOT}/runtime"
PY_DIR="${DEST}/python/${TARGET}"
UV_DIR="${DEST}/uv/${TARGET}"

case "${TARGET}" in
  *windows*)
    PY_BIN="${PY_DIR}/python.exe"
    UV_BIN_NAME="uv.exe"
    UV_ARCHIVE_EXT="zip"
    ;;
  *)
    PY_BIN="${PY_DIR}/bin/python3"
    UV_BIN_NAME="uv"
    UV_ARCHIVE_EXT="tar.gz"
    ;;
esac
UV_BIN="${UV_DIR}/${UV_BIN_NAME}"

if [[ "${FORCE}" -eq 1 ]]; then
  rm -rf "${PY_DIR}" "${UV_DIR}"
fi

mkdir -p "${PY_DIR}" "${UV_DIR}"

# ---------- python-build-standalone ----------

if [[ -x "${PY_BIN}" ]]; then
  echo "python already present: ${PY_BIN}"
else
  PBS_FILE="cpython-${PYTHON_VERSION}+${PBS_RELEASE}-${TARGET}-install_only.tar.gz"
  PBS_URL="https://github.com/astral-sh/python-build-standalone/releases/download/${PBS_RELEASE}/${PBS_FILE}"
  echo "fetching ${PBS_FILE}"
  TMP_PY="$(mktemp -d)"
  trap 'rm -rf "${TMP_PY}"' EXIT
  curl -fsSL --retry 3 -o "${TMP_PY}/py.tar.gz" "${PBS_URL}"
  # python-build-standalone tarballs contain a `python/` top-level dir;
  # strip it so contents land directly in <triple>/.
  tar -xzf "${TMP_PY}/py.tar.gz" -C "${PY_DIR}" --strip-components=1
  rm -rf "${TMP_PY}"
  trap - EXIT
  echo "installed python: ${PY_BIN}"

  # Prune tkinter / tcl / tk — bots run headless, never import them, and
  # `linuxdeploy` chokes on `_tkinter.so → libtcl9.0.so` when the host
  # has no system tcl/tk during AppImage bundling. Also drops idlelib
  # and turtledemo which depend on tkinter.
  echo "pruning tkinter/tcl/tk from python tree"
  find "${PY_DIR}" \
    \( -name "_tkinter*.so" \
    -o -name "_tkinter*.pyd" \
    -o -name "libtcl*.so*" \
    -o -name "libtk*.so*" \
    -o -name "tcl[0-9]*.dll" \
    -o -name "tk[0-9]*.dll" \
    \) -delete 2>/dev/null || true
  for sub in tkinter idlelib turtledemo; do
    find "${PY_DIR}" -type d -name "$sub" -exec rm -rf {} + 2>/dev/null || true
  done
  for prefix in tcl tk itcl tdbc thread tcllib tklib; do
    find "${PY_DIR}/lib" -maxdepth 2 -type d -name "${prefix}*" -exec rm -rf {} + 2>/dev/null || true
  done
fi

# ---------- uv ----------

if [[ -x "${UV_BIN}" ]]; then
  echo "uv already present: ${UV_BIN}"
else
  UV_FILE="uv-${TARGET}.${UV_ARCHIVE_EXT}"
  UV_URL="https://github.com/astral-sh/uv/releases/download/${UV_VERSION}/${UV_FILE}"
  echo "fetching ${UV_FILE}"
  TMP_UV="$(mktemp -d)"
  trap 'rm -rf "${TMP_UV}"' EXIT
  curl -fsSL --retry 3 -o "${TMP_UV}/${UV_FILE}" "${UV_URL}"
  case "${UV_ARCHIVE_EXT}" in
    tar.gz)
      tar -xzf "${TMP_UV}/${UV_FILE}" -C "${TMP_UV}" --strip-components=1
      ;;
    zip)
      if ! command -v unzip >/dev/null 2>&1; then
        echo "unzip not found on PATH; needed for windows targets" >&2
        exit 1
      fi
      unzip -q "${TMP_UV}/${UV_FILE}" -d "${TMP_UV}/extracted"
      # uv zip layout: uv-<triple>/uv.exe
      cp "${TMP_UV}/extracted"/uv-*/* "${TMP_UV}/" || cp "${TMP_UV}/extracted"/* "${TMP_UV}/"
      ;;
  esac
  cp "${TMP_UV}/${UV_BIN_NAME}" "${UV_BIN}"
  chmod +x "${UV_BIN}" 2>/dev/null || true
  # uvx ships alongside uv — copy it too if present, since some uv
  # subcommands shell out to uvx.
  if [[ -f "${TMP_UV}/uvx" ]]; then
    cp "${TMP_UV}/uvx" "${UV_DIR}/uvx"
    chmod +x "${UV_DIR}/uvx" 2>/dev/null || true
  fi
  if [[ -f "${TMP_UV}/uvx.exe" ]]; then
    cp "${TMP_UV}/uvx.exe" "${UV_DIR}/uvx.exe"
  fi
  rm -rf "${TMP_UV}"
  trap - EXIT
  echo "installed uv: ${UV_BIN}"
fi

echo
echo "Runtime ready for ${TARGET}"
echo "  python: ${PY_BIN}"
echo "  uv:     ${UV_BIN}"
