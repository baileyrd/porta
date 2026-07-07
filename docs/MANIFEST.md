# The tool manifest

Everything `porta install` knows how to install is described declaratively
in TOML. Two manifests are merged at load time:

1. **Built-in** — [`manifests/tools.toml`](../manifests/tools.toml),
   compiled into the `porta` binary at build time (`include_str!`), so the
   binary works with no data files on disk.
2. **User** — `~/.porta/tools.toml` (more precisely `$PORTA_HOME/tools.toml`),
   optional. Entries here are merged over the built-ins **by `name`**: a
   matching name replaces the built-in entry wholesale; a new name is
   appended. This is how you add tools or repoint a built-in at a different
   version without forking porta.

After merging, every tool must declare at least one install strategy or the
manifest is rejected with an error naming the offending tool.

## Tool entry

```toml
[[tool]]
name = "ripgrep"          # required; the `porta install <name>` key
display_name = "ripgrep"  # optional; used in human-facing messages
description = "Fast recursive search"  # optional; shown by `porta list`

[tool.script]   # zero or one of each strategy section
[tool.binary]
[tool.source]
```

## Strategy: `script` — run the vendor's official installer

For tools whose vendor already ships a trustworthy, portable, no-admin
installer (Claude Code is the motivating case). porta downloads the script
over HTTPS, writes it under `~/.porta/cache/installer-scripts/`, marks it
`0700` (Unix), and runs it with the interpreter you name. porta does **not**
copy the resulting binary into `~/.porta/bin` — the vendor's installer owns
its install directory and usually its own update mechanism — instead the
`installs_to` directory is added to PATH alongside porta's own `bin/`.

```toml
[tool.script]
installs_to = "~/.local/bin"   # where the vendor's installer puts the binary
                               # (leading ~ is expanded); added to PATH

[tool.script.unix]             # used on Linux + macOS; omit if unsupported
url = "https://claude.ai/install.sh"
interpreter = "bash"
# args = [...]                 # optional; see placeholder note below

[tool.script.windows]          # used on Windows; omit if unsupported
url = "https://claude.ai/install.ps1"
interpreter = "powershell"
args = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "{script}"]
```

The literal `{script}` placeholder in `args` is replaced with the downloaded
script's path — needed when the interpreter requires the path in a specific
position (PowerShell's `-File`). If no `{script}` placeholder appears, the
path is appended after `args`, which is right for `bash <path>`.

`uninstall` of a `script` tool only removes porta's record; porta never
deletes a vendor-managed installation.

## Strategy: `binary` — prebuilt release archive

Downloads an archive for the current platform, extracts it, and copies the
named binary to `~/.porta/bin/<name>` (`<name>.exe` on Windows), setting the
executable bit. Downloads are cached in `~/.porta/cache/<name>/<version>/`,
so reinstalling the same version is offline.

```toml
[tool.binary]
version = "14.1.1"             # required; recorded in state.json and used
                               # as the cache key

[tool.binary.targets.linux-x86_64]
url = "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz"
archive = "tar.gz"             # "tar.gz" | "zip" | "raw"
binary_path = "ripgrep-14.1.1-x86_64-unknown-linux-musl/rg"
```

- **Target keys** are `<os>-<arch>` from Rust's `std::env::consts::{OS,ARCH}`:
  `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`,
  `windows-x86_64`, `windows-aarch64`. Installing on a platform with no
  matching key is an error naming the keys that *do* exist (and triggers the
  source fallback if a `source` section is declared).
- **`archive = "raw"`** means the downloaded file *is* the binary — no
  extraction (common for single-file Go/Rust tool releases).
- **`binary_path`** is the path inside the extracted archive. If the exact
  path doesn't exist (archives often nest under a top-level directory whose
  name embeds a version/target string that's hard to predict), porta searches
  one directory level down for either the relative path or the bare file
  name — so a slightly-wrong prefix still resolves.

## Strategy: `source` — clone and build

Shallow-clones a git repository, runs a build command inside it, and copies
the produced binary into `~/.porta/bin/`. porta checks the build tool (the
first element of `build_cmd`) is on PATH *before* cloning, so you get a
clear error instead of a wasted checkout. porta does not install compilers;
the toolchain must already be there.

```toml
[tool.source]
repo = "https://github.com/BurntSushi/ripgrep"
git_ref = "14.1.1"            # optional tag/branch; also recorded as the
                              # installed "version" (else "source")
build_cmd = ["cargo", "build", "--release", "--bin", "rg"]
binary_path = "target/release/rg"   # relative to the repo root
```

The checkout lives at `~/.porta/tools/<name>-src` during the build and is
recreated from scratch on each install (no incremental state to go stale).

## Strategy selection

For a tool declaring multiple strategies, `porta install <name>`:

1. uses `script` if declared — the vendor's own installer is authoritative;
2. else tries `binary`; on any failure (no target for this platform,
   download or extraction error) falls back to `source` if declared,
   printing the binary failure first.

`--strategy script|binary|source` forces one, erroring if the tool doesn't
declare it.

## What's in the built-in manifest today

| name | strategies | what it is |
|---|---|---|
| `ai` | script | Claude Code, via Anthropic's official native installer (`https://claude.ai/install.sh` / `install.ps1`) — the AI CLI the whole environment is built around |
| `ripgrep` | binary + source | fast recursive search; doubles as the worked example of a two-strategy entry |

The built-in list is intentionally minimal: entries ship compiled into the
binary, so each one is a maintenance commitment (release URLs go stale).
The expectation is that machine- or team-specific tools live in
`~/.porta/tools.toml`. Good candidates are any single-binary CLI with
GitHub-style release archives (`fd`, `fzf`, `bat`, `jq`, `just`, `deno`,
`uv`, ...) or anything you'd build with one `cargo`/`go` command.

## Worked example: adding `fd` on your machine

```toml
# ~/.porta/tools.toml
[[tool]]
name = "fd"
description = "A simple, fast alternative to find"

[tool.binary]
version = "10.2.0"

[tool.binary.targets.linux-x86_64]
url = "https://github.com/sharkdp/fd/releases/download/v10.2.0/fd-v10.2.0-x86_64-unknown-linux-musl.tar.gz"
archive = "tar.gz"
binary_path = "fd-v10.2.0-x86_64-unknown-linux-musl/fd"

[tool.source]
repo = "https://github.com/sharkdp/fd"
git_ref = "v10.2.0"
build_cmd = ["cargo", "build", "--release"]
binary_path = "target/release/fd"
```

Then:

```
$ porta list          # fd now appears alongside the built-ins
$ porta install fd    # binary first, source fallback
$ porta which fd
/home/you/.porta/bin/fd
```
