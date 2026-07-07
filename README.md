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
user `PATH`, and downloads [Claude Code](https://claude.com/claude-code)
(checksum-verified, from Anthropic's official release endpoint) into
`~/.porta/bin/claude` — inside the environment, so it travels with it.
Restart your shell afterwards (or run `porta path` to print the PATH line
to source immediately).

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
  - **`binary`** — downloads a prebuilt release (raw binary or archive) for
    your OS/arch, verifies its SHA-256 when the manifest publishes one, and
    places the binary in `~/.porta/bin`. This is how the bundled AI CLI is
    installed: `ai` pulls the real `claude` binary from the same release
    endpoint Anthropic's official installer uses
    (`downloads.claude.ai/claude-code-releases`), checksum-verified, into
    `~/.porta/bin/claude` — fully inside the environment, so it travels
    with it.
  - **`source`** — `git clone`s the tool and builds it locally (e.g. via
    `cargo build --release`). Requires the relevant toolchain.
  - **`script`** — runs a tool's own official no-admin installer, for tools
    where that's the only sensible path. Note these installs land wherever
    the vendor's installer puts them (outside `~/.porta`), so they are the
    one thing that does *not* travel when you copy the environment — prefer
    `binary` for anything that must stay portable.

  A tool can declare both `binary` and `source`; `porta install` tries the
  binary first and automatically falls back to building from source if no
  prebuilt asset matches your platform (or the download fails). Force one
  explicitly with `--strategy binary|source|script`.
- **`porta doctor`** — environment status: is `~/.porta/bin` on `PATH`,
  what's installed and how.
- **`porta list`** — every tool in the manifest, and whether it's installed.
- **`porta uninstall <name>`** — removes a `binary`/`source` install. For
  `script`-installed tools this only forgets porta's own record of it —
  porta never deletes a vendor's own installation, since that installer
  manages its own updates/uninstall.
- **`porta which <name>`** / **`porta path`** — small utilities.

## Where things land

| What | Where |
|---|---|
| porta's home | `~/.porta` (macOS/Linux) / `%LOCALAPPDATA%\porta` (Windows); override with `$PORTA_HOME` |
| `binary`/`source` tool installs | `~/.porta/bin/<tool>` — the one directory porta puts on PATH (the `ai` tool installs as `~/.porta/bin/claude`) |
| download cache | `~/.porta/cache/<tool>/<version>/` (reinstalling a cached version is offline) |
| source-build checkouts | `~/.porta/tools/<tool>-src` (recreated per install) |
| install registry | `~/.porta/state.json` (paths stored relative to `$PORTA_HOME`, so the file survives a move) |
| `script` tool installs | wherever the vendor's installer puts them — the one non-portable exception |

## Moving the environment to another machine

The environment is a single directory, so moving it is a copy:

```sh
# on the old machine
tar -czf porta-env.tar.gz -C ~ .porta

# on the new machine (same OS/arch)
tar -xzf porta-env.tar.gz -C ~
~/.porta/bin/porta init      # wires PATH on the new machine
```

Everything installed via `binary`/`source` — including the `claude` binary —
comes along, and `state.json` records locations relative to `$PORTA_HOME`,
so `porta doctor`/`which`/`uninstall` keep working even when the new
machine's home directory has a different path.

Caveats:

- The copy is per-OS/arch: a `~/.porta` built on x86-64 Linux won't run on
  an ARM Mac. Re-run the installs there instead (`porta install ai`).
- Claude Code's *login and settings* live in `~/.claude` (or
  `$CLAUDE_CONFIG_DIR`), not inside `~/.porta` — copy that directory too if
  you want to carry your session, or just log in again on the new machine.
- porta's copy of `claude` is version-pinned at install time. Update it
  with `porta install ai` (which re-resolves the latest release); Claude
  Code's own background auto-updater targets the vendor layout in
  `~/.local`, not porta's copy, so consider setting `DISABLE_AUTOUPDATER=1`
  in [Claude Code settings](https://code.claude.com/docs/en/settings) to
  silence it.

## Documentation

- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** — module map, install
  flow, PATH-wiring mechanics, TLS trust policy, design trade-offs.
- **[docs/MANIFEST.md](docs/MANIFEST.md)** — full manifest schema reference:
  every field of the `script`/`binary`/`source` strategies, target keys,
  merge rules, and a worked example of adding your own tool.

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

Deleting `~/.porta` removes everything porta installed, including its copy
of Claude Code. Two things live outside it: Claude Code's own settings and
login (`~/.claude`, `~/.claude.json` — see
[Claude Code's uninstall docs](https://code.claude.com/docs/en/setup#uninstall-claude-code)),
and anything you installed via the `script` strategy, which the vendor's
own uninstaller manages.

## Building from source

```sh
git clone https://github.com/baileyrd/porta
cd porta
cargo build --release
cargo test
```

Requires a stable Rust toolchain (2021 edition).
