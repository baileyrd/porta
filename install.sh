#!/usr/bin/env bash
# porta bootstrap installer (macOS, Linux, WSL).
#
#   curl -fsSL https://raw.githubusercontent.com/baileyrd/porta/main/install.sh | bash
#
# Everything this script does is scoped to the current user — it never asks
# for sudo and never touches a system directory:
#   - installs the `porta` binary into $PORTA_HOME/bin (default ~/.porta/bin)
#   - if no prebuilt release is available for this platform yet, builds
#     porta from source, installing a user-local Rust toolchain via rustup
#     first if one isn't already on PATH
#   - runs `porta init` to wire $PORTA_HOME/bin onto PATH for future shells
#
# Optional: `curl -fsSL .../install.sh | bash -s -- <version>` installs a
# specific tagged release instead of the latest one.
set -euo pipefail

REPO="baileyrd/porta"
VERSION="${1:-latest}"
PORTA_HOME="${PORTA_HOME:-$HOME/.porta}"
BIN_DIR="$PORTA_HOME/bin"

log() { printf 'porta-install: %s\n' "$*"; }
die() { printf 'porta-install: error: %s\n' "$*" >&2; exit 1; }

detect_os() {
  case "$(uname -s)" in
    Linux) echo linux ;;
    Darwin) echo macos ;;
    *) die "unsupported OS: $(uname -s) (native install only covers Linux/macOS; try WSL on Windows)" ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo x86_64 ;;
    arm64|aarch64) echo aarch64 ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
}

# Tries to fetch a prebuilt release archive; returns 1 (without dying) if
# none exists yet so the caller can fall back to building from source.
try_install_prebuilt() {
  local os="$1" arch="$2" tag="$3"
  local asset="porta-${os}-${arch}.tar.gz"
  local url
  if [ "$tag" = "latest" ]; then
    url="https://github.com/$REPO/releases/latest/download/$asset"
  else
    url="https://github.com/$REPO/releases/download/$tag/$asset"
  fi

  local tmp
  tmp="$(mktemp -d)"

  # Deliberately not a `trap ... RETURN` here: that trap stays armed for
  # every later function return in the same shell (a well-known bash
  # gotcha), not just this one, so cleanup is done explicitly at each exit
  # point instead.
  if ! curl -fsSL -o "$tmp/$asset" "$url" 2>/dev/null; then
    rm -rf "$tmp"
    return 1
  fi

  log "found prebuilt release ($asset), extracting..."
  mkdir -p "$BIN_DIR"
  tar -xzf "$tmp/$asset" -C "$tmp"
  install -m 0755 "$tmp/porta" "$BIN_DIR/porta"
  rm -rf "$tmp"
  return 0
}

ensure_rust_toolchain() {
  if command -v cargo >/dev/null 2>&1; then
    return 0
  fi
  log "no Rust toolchain found; installing one for your user via rustup (no admin needed)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
}

build_from_source() {
  local tag="$1"
  ensure_rust_toolchain
  command -v git >/dev/null 2>&1 || die "git is required to build porta from source"

  local src_dir="$PORTA_HOME/src/porta"
  rm -rf "$src_dir"
  mkdir -p "$(dirname "$src_dir")"

  log "cloning $REPO..."
  if [ "$tag" = "latest" ]; then
    git clone --depth 1 "https://github.com/$REPO" "$src_dir"
  else
    git clone --depth 1 --branch "$tag" "https://github.com/$REPO" "$src_dir"
  fi

  log "building porta (this can take a minute the first time)..."
  (cd "$src_dir" && cargo build --release)

  mkdir -p "$BIN_DIR"
  install -m 0755 "$src_dir/target/release/porta" "$BIN_DIR/porta"
  rm -rf "$src_dir"
}

main() {
  local os arch
  os="$(detect_os)"
  arch="$(detect_arch)"

  mkdir -p "$BIN_DIR"

  if ! try_install_prebuilt "$os" "$arch" "$VERSION"; then
    log "no prebuilt binary for $os-$arch (or release '$VERSION' not found); building from source instead"
    build_from_source "$VERSION"
  fi

  log "porta installed at $BIN_DIR/porta"

  if [ "${PORTA_SKIP_AI:-0}" = "1" ]; then
    "$BIN_DIR/porta" init
  else
    "$BIN_DIR/porta" init --with-ai
  fi

  log "done. Restart your shell, or run: source <($BIN_DIR/porta path 2>/dev/null || true)"
}

main "$@"
