# porta

[![ci](https://github.com/baileyrd/porta/actions/workflows/ci.yml/badge.svg)](https://github.com/baileyrd/porta/actions/workflows/ci.yml)

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
curl -fsSL https://raw.githubusercontent.com/baileyrd/porta/main/install.sh | sh
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

### Private repositories

GitHub answers anonymous requests for a **private** repository with
`404: Not Found` — not `403` — so if this repo (or a private tool repo in
the manifest, like `rush`) hasn't been made public, the installers and
`porta install` will report 404s for release assets, source archives, and
raw files. Fix: set a token with read access to the repo before running
them:

```sh
export GITHUB_TOKEN=<fine-grained PAT with contents:read>   # macOS/Linux
```

```powershell
$env:GITHUB_TOKEN = "<PAT>"                                  # Windows
```

Both installers and porta's own downloader pick it up (`GH_TOKEN` works
too) and attach it **only** to requests bound for GitHub's own hosts
(`github.com`, `codeload.github.com`, `raw.githubusercontent.com`, …) —
never to any other endpoint, like rustup or Anthropic's release host.

One wrinkle while the repo is private: the `curl`/`irm` quickstart URLs
above point at `raw.githubusercontent.com`, which 404s anonymously too —
download `install.sh`/`install.ps1` some other way first (e.g. from a
clone, or the GitHub UI), then run it with the token set.

## What it does

- **`porta init [--home <dir>]`** — creates the environment layout and adds
  its `bin/` to your `PATH` (an idempotent block in `.profile`/`.bashrc`/
  `.zshrc`/fish's `config.fish` on Unix; your user environment block,
  `HKCU\Environment`, on Windows — never the machine-wide PATH). `--home`
  puts the environment wherever you want it (see
  [Choosing where it lives](#choosing-where-it-lives-and-moving-it-later)).
- **`porta move <dir>`** — relocate an existing environment: moves the
  directory, re-wires PATH (removing the stale entry), re-links dotfiles.
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
  - **`source`** — downloads the tool's source tarball (no `git` needed on
    the host) — or `git clone`s when no `archive_url` is declared — and
    builds it locally (e.g. via `cargo build --release`). Requires the
    build toolchain named in the entry; that's checked with a clear error,
    never assumed.
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
- **`porta dotfiles add|link|list|remove`** — keep dotfiles *inside* the
  environment so they travel with it (see [Dotfiles](#dotfiles)).
- **`porta which <name>`** / **`porta path`** — small utilities.

## Built-in tools

| name | installs | how |
|---|---|---|
| `ai` | `claude` ([Claude Code](https://claude.com/claude-code)) | checksum-verified binary from Anthropic's release endpoint; `latest` resolved at install time |
| `gh` | `gh` ([GitHub CLI](https://cli.github.com)) | release archive, SHA-256-verified against gh's combined `checksums.txt` (pinned version — bump in the manifest) |
| `rush` | `rush` ([a bash-compatible shell in Rust](https://github.com/baileyrd/rush)) | source tarball → `cargo build` — no git needed |
| `ripgrep` | `rg` | release archive, with a build-from-source fallback |

`porta list` shows the same list plus anything you've added, and
[Extending the manifest](#extending-the-manifest) covers adding your own.

## Where things land

| What | Where |
|---|---|
| porta's home | anywhere you choose (`porta init --home <dir>`); defaults to `~/.porta` (macOS/Linux) / `%LOCALAPPDATA%\porta` (Windows) |
| `binary`/`source` tool installs | `~/.porta/bin/<tool>` — the one directory porta puts on PATH (the `ai` tool installs as `~/.porta/bin/claude`) |
| download cache | `~/.porta/cache/<tool>/<version>/` (reinstalling a cached version is offline) |
| source-build checkouts | `~/.porta/tools/<tool>-src` (recreated per install) |
| install registry | `~/.porta/state.json` (paths stored relative to `$PORTA_HOME`, so the file survives a move) |
| tracked dotfiles | `~/.porta/dotfiles/<home-relative-path>`, symlinked into `$HOME` |
| `script` tool installs | wherever the vendor's installer puts them — the one non-portable exception |

## Choosing where it lives (and moving it later)

The environment's root doesn't have to be the default — designate any
directory at setup time:

```sh
porta init --home /opt/tools/porta        # Unix
porta init --home C:\tools\porta          # Windows
```

`--home` creates the layout there, **copies the porta binary inside**
(`<home>/bin/porta`), and wires PATH to it. From then on no environment
variable is needed: a porta binary running from `<anything>/bin/` next to a
`state.json` treats that directory as its home — the folder is
self-describing, which is also why you can copy it between machines. The
full resolution order is `$PORTA_HOME` (explicit override) → the
executable's own location → the platform default; `porta doctor` shows
which one applied.

Already set up and want it elsewhere? Move it in place:

```sh
porta move D:\tools\porta
```

`move` relocates the directory (handling cross-drive moves by copy),
rewrites the PATH block — dropping the old entry, including from the
Windows user PATH — and re-points every tracked dotfile symlink at the
moved store. If you had `$PORTA_HOME` exported, porta reminds you to update
or drop it.

To designate the location at bootstrap time, set `PORTA_HOME` just for the
install command — it's only needed that once, since the installed binary
self-locates afterwards:

```sh
PORTA_HOME=/opt/tools/porta sh install.sh
```

## Dotfiles

Binaries aren't the whole environment — your configuration is the other
half. `porta dotfiles` stores config files inside `~/.porta/dotfiles` and
symlinks them into place, so they move with the environment:

```sh
porta dotfiles add ~/.gitconfig ~/.bashrc ~/.config/nvim
porta dotfiles list      # tracked files + link status
porta dotfiles remove ~/.bashrc   # restore a real copy, stop tracking
```

`add` *moves* the file into the store and leaves a symlink at the original
path, so everything keeps working exactly as before — editing `~/.gitconfig`
edits the stored copy. On a new machine, `porta init` (or `porta dotfiles
link`) recreates every symlink; if the machine already has its own version
of a file, it's preserved as `<name>.porta-backup`, never overwritten.

Directories work too (`~/.config/nvim` above). On Windows, symlinks without
admin need Developer Mode enabled; when unavailable porta places a copy
instead and tells you so.

A nice side effect for shell config: the PATH block porta writes uses
`$HOME`-relative paths (`export PATH="$HOME/.porta/bin:$PATH"`), so a
tracked `.bashrc`/`.profile` is portable as-is — no per-machine rewrite
needed.

## Bring your own shell

A shell is just another binary, so it can live in the environment like
everything else — and one ships in the built-in manifest:
[rush](https://github.com/baileyrd/rush), a small bash-compatible shell in
Rust.

```sh
porta install rush     # source tarball -> cargo build -> ~/.porta/bin/rush
```

No `git` is needed (the entry downloads a source tarball); it does need
`cargo` on PATH, which the bootstrap installer sets up when it builds porta
from source. The shell then travels with the environment like any other
`binary`/`source` tool.

**The honest caveat: your *login* shell can't be changed without admin.**
`chsh` only accepts shells listed in `/etc/shells`, and editing that file
requires root. The no-admin patterns instead:

- **Exec into it from your rc file** — add to `.bashrc` (ideally one you
  track with `porta dotfiles`):

  ```sh
  # hand the session to the porta-managed shell when it's available
  if [ -x "$HOME/.porta/bin/rush" ] && [ -z "$PORTA_SHELL" ]; then
      export PORTA_SHELL=1
      exec "$HOME/.porta/bin/rush"
  fi
  ```

- **Point your terminal at it** — most terminal emulators let you set a
  custom command/profile per user, no admin needed (e.g. set the profile's
  command to `~/.porta/bin/rush`).

Either way the login shell on record stays `bash`, but every interactive
session lands in your shell — and both the shell binary and the rc file
that launches it travel with `~/.porta`.

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
comes along; `porta init` also re-links every tracked dotfile. `state.json`
records locations relative to `$PORTA_HOME`, so `porta doctor`/`which`/
`uninstall` keep working even when the new machine's home directory has a
different path.

Caveats:

- The copy is per-OS/arch: a `~/.porta` built on x86-64 Linux won't run on
  an ARM Mac. Re-run the installs there instead (`porta install ai`).
- Claude Code's *login and settings* live in `~/.claude` (or
  `$CLAUDE_CONFIG_DIR`), not inside `~/.porta`. To carry them with the
  environment, track them: `porta dotfiles add ~/.claude` (mind that this
  moves your credentials into the portable directory — treat the copy like
  the secret it is). Otherwise just log in again on the new machine.
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
- **Nothing on the host is assumed present.** Archive extraction
  (`tar.gz`/`zip`) is built into the porta binary with pure-Rust
  decompressors and path-traversal guards — no host `tar`, `unzip`, or
  `Expand-Archive` is ever invoked. Source installs download release
  tarballs, so no `git` is needed (entries without an `archive_url` fall
  back to `git clone`, with a clear error if git is missing). The bootstrap
  script is POSIX `sh` (no bash — stock Alpine works) and needs only a
  downloader: `curl` or `wget`, whichever exists.
- **`porta` doesn't manage compilers.** The `source` install strategy
  requires the build tool named in the entry (`cargo`, etc.) on `PATH` —
  checked up front with a clear error. The bootstrap installer
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
