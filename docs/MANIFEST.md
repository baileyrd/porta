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
bin_name = "rg"           # optional; installed command name when it differs
                          # from `name` (the `ai` tool installs `claude`);
                          # defaults to `name`
description = "Fast recursive search"  # optional; shown by `porta list`

[tool.script]   # zero or one of each strategy section
[tool.binary]
[tool.source]
```

## Strategy: `script` — run the vendor's official installer

For tools whose vendor ships a trustworthy no-admin installer and where a
direct binary download isn't practical. porta downloads the script over
HTTPS, writes it under `~/.porta/cache/installer-scripts/`, marks it `0700`
(Unix), and runs it with the interpreter you name. porta does **not** copy
the resulting binary into `~/.porta/bin` — the vendor's installer owns its
install directory and usually its own update mechanism — instead the
`installs_to` directory is added to PATH alongside porta's own `bin/`.

**Portability caveat:** because the install lands outside `$PORTA_HOME`,
`script` tools do not travel when you copy the environment to another
machine — they must be reinstalled there. Prefer `binary` when a tool
publishes raw binaries or archives (the built-in `ai` entry switched from
`script` to `binary` for exactly this reason; the example below shows what
its script form looked like).

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
version = "14.1.1"             # a pinned version, or "latest" (see below);
                               # recorded in state.json, used as cache key
# version_url = "https://..."  # only with version = "latest": GET this URL
                               # to resolve the current version string at
                               # install time

[tool.binary.targets.linux-x86_64]
url = "https://github.com/BurntSushi/ripgrep/releases/download/{version}/ripgrep-{version}-x86_64-unknown-linux-musl.tar.gz"
archive = "tar.gz"             # "tar.gz" | "zip" | "raw"
binary_path = "ripgrep-14.1.1-x86_64-unknown-linux-musl/rg"
# checksum = { url = "https://.../{version}/manifest.json", json_path = "platforms.linux-x64.checksum" }
```

- **Target keys** are `<os>-<arch>` from Rust's `std::env::consts::{OS,ARCH}`:
  `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`,
  `windows-x86_64`, `windows-aarch64`. Installing on a platform with no
  matching key is an error naming the keys that *do* exist (and triggers the
  source fallback if a `source` section is declared).
- **Dynamic versions.** With `version = "latest"` and a `version_url`, porta
  GETs the URL at install time, validates the body looks like a version
  string, and substitutes it for every `{version}` placeholder in `url` and
  `checksum.url`. A pinned `version` never touches the network and is also
  substituted into `{version}` placeholders — so a user manifest can pin an
  entry that's `latest` in the built-ins just by overriding `version`.
- **`archive = "raw"`** means the downloaded file *is* the binary — no
  extraction (common for single-file releases; this is how `ai` downloads
  `claude`). `binary_path` is ignored for raw downloads.
- **`binary_path`** is the path inside the extracted archive. If the exact
  path doesn't exist (archives often nest under a top-level directory whose
  name embeds a version/target string that's hard to predict), porta searches
  one directory level down for either the relative path or the bare file
  name — so a slightly-wrong prefix still resolves.
- **`checksum`** enables SHA-256 verification of the downloaded file before
  anything is installed, and re-verifies cache hits so a corrupted cache can
  never be installed. `url` points at the checksum document (`{version}`
  templated like the download URL). With `json_path`, the document is JSON
  and the dotted path locates the hex digest (e.g.
  `platforms.linux-x64.checksum` in Claude Code's release manifest).
  Without it, the document is `sha256sum`-format text: for a combined
  `checksums.txt` listing every release asset (gh and most goreleaser
  projects ship one), porta picks the line whose filename matches the
  downloaded asset (tolerating the `*` binary-mode marker), and errors
  rather than guesses if nothing matches; a single-line `.sha256` document
  just uses its first token.

## Strategy: `source` — fetch source and build

Obtains a tool's source, runs a build command inside it, and copies the
produced binary into `~/.porta/bin/`. porta checks the build tool (the
first element of `build_cmd`) is on PATH *before* fetching anything, so you
get a clear error instead of a wasted download. porta does not install
compilers; the toolchain must already be there.

With an **`archive_url`**, the source arrives as a `.tar.gz` download
extracted by porta's built-in decompressor — **no `git` on the host**.
`{ref}` is replaced with `git_ref`, and forge tarballs' single top-level
directory (`<repo>-<ref>/`) is detected automatically. Without an
`archive_url`, porta falls back to `git clone --depth 1` and requires git
on PATH (checked, with an error pointing at the fix).

```toml
[tool.source]
repo = "https://github.com/BurntSushi/ripgrep"
git_ref = "14.1.1"            # optional tag/branch; also recorded as the
                              # installed "version" (else "source")
archive_url = "https://codeload.github.com/BurntSushi/ripgrep/tar.gz/refs/tags/{ref}"
build_cmd = ["cargo", "build", "--release", "--bin", "rg"]
binary_path = "target/release/rg"   # relative to the source root
```

(`codeload.github.com` is GitHub's archive server — the host that
`github.com/<owner>/<repo>/archive/...` URLs redirect to; using it directly
avoids the redirect. Branch form: `.../tar.gz/refs/heads/{ref}`.)

**Private GitHub repos:** GitHub answers anonymous requests for private
repos with 404, so an entry pointing at one (like the built-in `rush`)
needs `GITHUB_TOKEN`/`GH_TOKEN` set. porta attaches the token to every
download whose URL is on a GitHub-owned host — `binary` URLs, `source`
archives, checksum documents alike — and never to any other host, so a
manifest entry can't leak your token elsewhere.

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
| `ai` | binary | Claude Code, downloaded checksum-verified from Anthropic's release endpoint (`downloads.claude.ai/claude-code-releases`, the same one the official installer uses) into `~/.porta/bin/claude` — inside the environment, so it moves with it. `version = "latest"` resolves at install time; re-run `porta install ai` to update. Linux targets use the glibc builds; on musl distros (Alpine), override with the `linux-*-musl` URLs in your user manifest. |
| `rush` | source | a small bash-compatible shell in Rust, built from its source tarball (needs `cargo`, not `git`) — a shell that lives inside, and travels with, the environment |
| `gh` | binary | the official GitHub CLI, from its release archives with SHA-256 verification against the combined `checksums.txt`. Pinned (GitHub has no plain-text latest-version endpoint); bump `version` to update. |
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
