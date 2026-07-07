#!/bin/sh
# porta bootstrap installer (macOS, Linux, WSL).
#
#   curl -fsSL https://raw.githubusercontent.com/baileyrd/porta/main/install.sh | sh
#
# Written for POSIX sh (dash/ash/busybox included) — no bash required.
# Everything it does is scoped to the current user; it never asks for sudo
# and never touches a system directory.
#
# Host requirements are kept to the floor and checked, never assumed:
#   - prebuilt-binary path: a downloader (curl OR wget) and sh. That's all —
#     releases ship the raw `porta` binary, so no tar/unzip is needed, and
#     once porta itself is installed it extracts every tool's archives with
#     its own built-in (pure-Rust) tar.gz/zip support.
#   - build-from-source fallback (no prebuilt release for this platform):
#     additionally tar (to unpack porta's source tarball — no git needed)
#     and a Rust toolchain; if cargo is missing, a user-local one is
#     installed via rustup (no admin).
#
# Optional: `... | sh -s -- <version>` installs a specific tagged release.
# Optional: PORTA_HOME=/opt/tools/porta designates a custom root — needed
# only for this one command; the installed binary self-locates afterwards.
# Optional: GITHUB_TOKEN=<PAT> if the porta repository is private (GitHub
# answers 404 to anonymous requests for private repos). Sent to GitHub's
# own hosts only, never anywhere else.
set -eu

REPO="baileyrd/porta"
VERSION="${1:-latest}"
PORTA_HOME="${PORTA_HOME:-$HOME/.porta}"
BIN_DIR="$PORTA_HOME/bin"

log() { printf 'porta-install: %s\n' "$*"; }
die() { printf 'porta-install: error: %s\n' "$*" >&2; exit 1; }

# fetch <url> <output-file>: curl or wget, whichever exists.
#
# GitHub serves anonymous 404s for private repositories, so when
# GITHUB_TOKEN (or GH_TOKEN) is set it is attached as a bearer token —
# but only to requests bound for GitHub's own hosts. It is never sent
# anywhere else (e.g. sh.rustup.rs).
fetch() {
  auth=""
  case "$1" in
    https://github.com/*|https://codeload.github.com/*|https://raw.githubusercontent.com/*|https://api.github.com/*|https://objects.githubusercontent.com/*|https://release-assets.githubusercontent.com/*)
      auth="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
      ;;
  esac
  if command -v curl >/dev/null 2>&1; then
    if [ -n "$auth" ]; then
      curl -fsSL -H "Authorization: Bearer $auth" -o "$2" "$1"
    else
      curl -fsSL -o "$2" "$1"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if [ -n "$auth" ]; then
      wget -q --header="Authorization: Bearer $auth" -O "$2" "$1"
    else
      wget -q -O "$2" "$1"
    fi
  else
    die "neither curl nor wget is available — one of them is required to download porta"
  fi
}

# verify_sha256 <file> <asset-name> <base-url>: check the download against
# the release's combined checksums.txt. Best-effort by design — a host
# without sha256sum/shasum, or a release without checksums.txt, skips with
# a note (verification must not raise the "assume nothing" host floor) —
# but an actual mismatch is fatal, never a silent fall-through.
verify_sha256() {
  vfile="$1"; vasset="$2"; vbase="$3"
  if command -v sha256sum >/dev/null 2>&1; then
    hasher="sha256sum"
  elif command -v shasum >/dev/null 2>&1; then
    hasher="shasum -a 256"
  else
    log "note: no sha256sum/shasum on this host; skipping checksum verification"
    return 0
  fi
  sums="$(mktemp)"
  if ! fetch "$vbase/checksums.txt" "$sums" 2>/dev/null; then
    rm -f "$sums"
    log "note: release publishes no checksums.txt; skipping checksum verification"
    return 0
  fi
  expected="$(awk -v name="$vasset" '$2 == name || $2 == ("*" name) {print $1; exit}' "$sums")"
  rm -f "$sums"
  if [ -z "$expected" ]; then
    log "note: checksums.txt has no entry for $vasset; skipping checksum verification"
    return 0
  fi
  actual="$($hasher "$vfile" | awk '{print $1}')"
  [ "$actual" = "$expected" ] \
    || die "checksum mismatch for $vasset: expected $expected, got $actual — refusing to install it"
  log "checksum verified ($vasset)"
}

detect_os() {
  case "$(uname -s)" in
    Linux) echo linux ;;
    Darwin) echo macos ;;
    *) die "unsupported OS: $(uname -s) (native install covers Linux/macOS; try WSL on Windows)" ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo x86_64 ;;
    arm64|aarch64) echo aarch64 ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
}

# Tries the prebuilt release paths; returns 1 (without dying) when no
# release asset exists so the caller can fall back to building from source.
try_install_prebuilt() {
  os="$1"; arch="$2"; tag="$3"
  if [ "$tag" = "latest" ]; then
    base="https://github.com/$REPO/releases/latest/download"
  else
    base="https://github.com/$REPO/releases/download/$tag"
  fi

  tmp="$(mktemp -d)"

  # Preferred asset: the raw binary — needs no extractor at all.
  if fetch "$base/porta-$os-$arch" "$tmp/porta" 2>/dev/null; then
    log "found prebuilt release binary (porta-$os-$arch)"
    verify_sha256 "$tmp/porta" "porta-$os-$arch" "$base"
    mkdir -p "$BIN_DIR"
    chmod 0755 "$tmp/porta"
    mv "$tmp/porta" "$BIN_DIR/porta"
    rm -rf "$tmp"
    return 0
  fi

  # Legacy/alternative asset shape: a tar.gz (only usable if tar exists).
  if command -v tar >/dev/null 2>&1 \
     && fetch "$base/porta-$os-$arch.tar.gz" "$tmp/porta.tar.gz" 2>/dev/null; then
    log "found prebuilt release archive (porta-$os-$arch.tar.gz), extracting..."
    tar -xzf "$tmp/porta.tar.gz" -C "$tmp"
    mkdir -p "$BIN_DIR"
    chmod 0755 "$tmp/porta"
    mv "$tmp/porta" "$BIN_DIR/porta"
    rm -rf "$tmp"
    return 0
  fi

  rm -rf "$tmp"
  return 1
}

ensure_rust_toolchain() {
  if command -v cargo >/dev/null 2>&1; then
    return 0
  fi
  log "no Rust toolchain found; installing one for your user via rustup (no admin needed)..."
  rustup_tmp="$(mktemp)"
  fetch "https://sh.rustup.rs" "$rustup_tmp"
  sh "$rustup_tmp" -y --no-modify-path
  rm -f "$rustup_tmp"
  # rustup's env file is POSIX-compatible.
  . "$HOME/.cargo/env"
}

# Builds porta from a source tarball — no git required.
build_from_source() {
  tag="$1"
  command -v tar >/dev/null 2>&1 \
    || die "building porta from source needs tar to unpack the source tarball (no prebuilt release exists for this platform yet)"
  ensure_rust_toolchain

  if [ "$tag" = "latest" ]; then
    src_url="https://codeload.github.com/$REPO/tar.gz/refs/heads/main"
  else
    src_url="https://codeload.github.com/$REPO/tar.gz/refs/tags/$tag"
  fi

  src_dir="$PORTA_HOME/src"
  rm -rf "$src_dir"
  mkdir -p "$src_dir"

  log "downloading porta source tarball ($src_url)..."
  fetch "$src_url" "$src_dir/porta-src.tar.gz"
  tar -xzf "$src_dir/porta-src.tar.gz" -C "$src_dir"
  rm -f "$src_dir/porta-src.tar.gz"

  # The tarball nests everything under porta-<ref>/ — find that directory.
  src_root=""
  for d in "$src_dir"/*/; do
    src_root="$d"
    break
  done
  [ -n "$src_root" ] || die "source tarball extraction produced no directory"

  log "building porta (this can take a minute the first time)..."
  (cd "$src_root" && cargo build --release)

  mkdir -p "$BIN_DIR"
  cp "$src_root/target/release/porta" "$BIN_DIR/porta"
  chmod 0755 "$BIN_DIR/porta"
  rm -rf "$src_dir"
}

main() {
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

  log "done. Restart your shell to pick up PATH (or run: eval \"\$($BIN_DIR/porta path)\")"
}

main "$@"
