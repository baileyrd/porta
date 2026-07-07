# porta

A portable developer environment you can stand up on any machine — locked
down, corporate-managed, freshly imaged, whatever — **without admin/root
privileges**, with an AI coding CLI ([Claude Code](https://claude.com/claude-code))
set up out of the box.

Everything porta does lives under a single per-user directory
(`~/.porta` on macOS/Linux, `%LOCALAPPDATA%\porta` on Windows). It never
writes to a system directory, never needs `sudo`/an elevated prompt, and
its own PATH changes are scoped to your user profile only.

## Quickstart

**macOS / Linux / WSL:**

```sh
curl -fsSL https://raw.githubusercontent.com/baileyrd/porta/main/install.sh | bash
```

**Windows (PowerShell, no WSL needed):**

```powershell
irm https://raw.githubusercontent.com/baileyrd/porta/main/install.ps1 | iex
```

This installs the `porta` CLI itself, wires its `bin/` directory onto your
user `PATH`, and installs [Claude Code](https://claude.com/claude-code) via
its own official native installer. Restart your shell afterwards (or run
`porta path` to print the PATH line to source immediately).

Don't want the AI CLI installed automatically? Set `PORTA_SKIP_AI=1` before
running the installer, and run `porta install ai` yourself whenever you want
it.

## What it does

- **`porta init`** — creates `~/.porta/{bin,tools,cache}` and adds
  `~/.porta/bin` to your `PATH` (an idempotent block in `.profile`/`.bashrc`/
  `.zshrc`/fish's `config.fish` on Unix; your user environment block,
  `HKCU\Environment`, on Windows — never the machine-wide PATH).
- **`porta install <name>`** — installs a tool from the
  [manifest](manifests/tools.toml) using whichever strategy it declares:
  - **`script`** — runs the tool's own official no-admin installer (this is
    how the bundled AI CLI, `ai` → Claude Code, is installed: porta just
    fetches and runs `https://claude.ai/install.sh` / `install.ps1`, exactly
    what [Anthropic's docs](https://code.claude.com/docs/en/setup) recommend,
    and makes sure the directory it installs into ends up on `PATH`).
  - **`binary`** — downloads a prebuilt release archive for your OS/arch and
    copies the binary into `~/.porta/bin`.
  - **`source`** — `git clone`s the tool and builds it locally (e.g. via
    `cargo build --release`). Requires the relevant toolchain.

  A tool can declare both `binary` and `source`; `porta install` tries the
  binary first and automatically falls back to building from source if no
  prebuilt asset matches your platform (or the download fails). Force one
  explicitly with `--strategy binary|source|script`.
- **`porta doctor`** — environment status: is `~/.porta/bin` on `PATH`,
  what's installed and how.
- **`porta list`** — every tool in the manifest, and whether it's installed.
- **`porta uninstall <name>`** — removes a `binary`/`source` install. For
  `script`-installed tools (like Claude Code) this only forgets porta's own
  record of it — porta never deletes a vendor's own installation, since that
  installer manages its own updates/uninstall.
- **`porta which <name>`** / **`porta path`** — small utilities.

## Extending the manifest

porta's built-in tool list lives in [`manifests/tools.toml`](manifests/tools.toml).
Add your own tools (or override a built-in entry) without forking porta by
writing `~/.porta/tools.toml` in the same format — entries there are merged
over the built-ins by `name`.

```toml
[[tool]]
name = "fd"
display_name = "fd"
description = "A simple, fast alternative to find"

[tool.binary]
version = "10.2.0"

[tool.binary.targets.linux-x86_64]
url = "https://github.com/sharkdp/fd/releases/download/v10.2.0/fd-v10.2.0-x86_64-unknown-linux-musl.tar.gz"
archive = "tar.gz"
binary_path = "fd-v10.2.0-x86_64-unknown-linux-musl/fd"
```

`[tool.binary.targets.<key>]` keys are `<os>-<arch>` (`linux-x86_64`,
`macos-aarch64`, `windows-x86_64`, ...) matching `std::env::consts::{OS,ARCH}`.
See [`manifests/tools.toml`](manifests/tools.toml) for a full worked example
(`ripgrep`) that declares both a `binary` and a `source` strategy.

## Design notes

- **No admin, anywhere.** Every install strategy places files under
  `$PORTA_HOME`; PATH updates touch only user-scoped configuration
  (`$HOME` rc files on Unix, `HKCU\Environment` on Windows via
  `[Environment]::SetEnvironmentVariable(..., 'User')` — the same mechanism
  tools like rustup use). Nothing in porta ever shells out to `sudo`, asks
  for an Administrator prompt, or writes to `/usr`, `/etc`, or
  `C:\Program Files`.
- **Pure-Rust TLS.** Downloads go through [`ureq`](https://github.com/algesten/ureq)
  with `rustls`, so porta doesn't depend on a system OpenSSL install. By
  default it trusts the same bundled root CAs your browser ships with
  (`ureq`'s own default) rather than the OS trust store, so a machine
  wouldn't silently start trusting a TLS-intercepting proxy just because
  porta happened to run there. If you're on a corporate network with a
  legitimate inspecting proxy and need porta to trust its root CA, set
  `PORTA_TRUST_SYSTEM_CERTS=1` to switch to platform certificate
  verification.
- **No vendored archive/compression code.** Extraction shells out to `tar`
  (present by default on Linux, macOS, and Windows 10 1803+) or, for `.zip`
  where `tar` can't handle it, `unzip` (Unix) / `Expand-Archive` (Windows) —
  all of which ship with the OS already.
- **`porta` doesn't manage compilers.** The `source` install strategy
  assumes the build tool it needs (`cargo`, etc.) is already on `PATH`; it
  won't install one for you mid-`porta install`. The bootstrap installer
  (`install.sh`/`install.ps1`) is the exception — it installs a user-local
  Rust toolchain via [rustup](https://rustup.rs) *just* to build `porta`
  itself, if no prebuilt release matches your platform yet.

## Uninstalling porta itself

```sh
rm -rf ~/.porta
# then remove the "# >>> porta initialize >>> ... # <<< porta initialize <<<"
# block porta added to your shell rc file(s).
```

Windows: delete `%LOCALAPPDATA%\porta` and remove porta's entry from your
user `Path` (`Settings → Environment Variables`, or
`[Environment]::SetEnvironmentVariable('Path', ..., 'User')`).

This doesn't touch tools installed via the `script` strategy (e.g. Claude
Code) — see [Claude Code's own uninstall instructions](https://code.claude.com/docs/en/setup#uninstall-claude-code)
for that.

## Building from source

```sh
git clone https://github.com/baileyrd/porta
cd porta
cargo build --release
cargo test
```

Requires a stable Rust toolchain (2021 edition).
