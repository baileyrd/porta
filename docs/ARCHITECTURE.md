# porta architecture

porta is a small Rust CLI with one job:
stand up a per-user development environment — including an AI coding CLI —
on a machine where you have no admin rights, and keep every side effect
inside user-owned locations.

## The one invariant

**Nothing porta does requires elevation.** Every code path writes only to:

| Location | Purpose |
|---|---|
| `$PORTA_HOME` (default `~/.porta`, Windows `%LOCALAPPDATA%\porta`) | all porta-managed files |
| `$HOME/.profile`, `.bashrc`, `.zshrc`, `~/.config/fish/config.fish` | PATH block (Unix) |
| `HKCU\Environment` (user scope, via PowerShell) | PATH entry (Windows) |

There is no `sudo`, no UAC prompt, no `/usr`, no `C:\Program Files`,
anywhere. This is checked most easily by grepping the source: porta never
spawns a privileged process and never writes outside the table above (the
`script` strategy delegates to a vendor installer, which is chosen for
having the same property — see below).

## Directory layout at runtime

```
~/.porta/
  bin/            <- installed tool binaries; the ONE dir porta puts on PATH
  cache/          <- downloaded archives, keyed <tool>/<version>/, so
                     re-installing the same version skips the network
    installer-scripts/   <- fetched vendor installer scripts before running
  tools/          <- scratch: source checkouts during `source` builds
  dotfiles/       <- tracked dotfiles, symlinked into $HOME (travel with
                     the environment; see the dotfiles module)
  state.json      <- registry of what porta installed (version/strategy/where)
                     + the list of tracked dotfiles
  tools.toml      <- OPTIONAL user manifest, merged over the built-in one
```

The root itself is designatable (`porta init --home <dir>`) and movable
(`porta move <dir>`). Resolution order in `paths.rs`: `$PORTA_HOME` env →
**executable self-location** (the binary runs from `<dir>/bin/` and
`<dir>/state.json` exists — the marker keeps a porta copied into, say,
`/usr/local/bin` from mistaking `/usr/local` for a home) → platform
default. `init` copies the running binary into `<home>/bin` and writes
`state.json`, which is what arms self-location; `porta doctor` reports
which rule resolved the home.

This layout is the portability unit: copy `~/.porta` to another machine
(same OS/arch), run `porta init` there, and the environment — including the
`claude` binary the `ai` tool installs — works. Two details make that hold:
`state.json` stores locations under `$PORTA_HOME` with a literal
`${PORTA_HOME}` prefix rather than an absolute path (expanded against the
*current* home on read), and the `ai` tool downloads the real binary into
`bin/` instead of delegating to a vendor installer. Configuration travels
too: `porta dotfiles add` moves files into `dotfiles/` and symlinks them
back into `$HOME`; `porta init` re-links them after a move (an existing
file on the new machine is preserved as `<name>.porta-backup`).

`script`-strategy tools are the deliberate exception to `bin/`: the vendor's
installer owns its install location, so porta records that directory in
`state.json` and adds it to PATH rather than copying the binary — copying
would break the vendor's update mechanism. The corollary is that `script`
installs live outside `$PORTA_HOME` and therefore do *not* travel with the
environment; they exist for tools where no direct download is practical.

## Module map

```
src/
  main.rs        clap dispatch; one cmd_* fn per subcommand
  cli.rs         clap derive types (the CLI surface, nothing else)
  paths.rs       every filesystem location, in one place; PORTA_HOME override
  manifest.rs    TOML schema (Tool/ScriptSpec/BinarySpec/SourceSpec),
                 built-in manifest embedded via include_str!, user-manifest
                 merge (by tool name), target-key resolution (os-arch)
  state.rs       state.json load/save; record/remove entries; the
                 tracked-dotfiles list
  dotfiles.rs    dotfile store: add (move + symlink), link (recreate all
                 symlinks, backing up what's in the way), list, remove
                 (restore a real copy). Unix symlinks; Windows falls back
                 to copies when symlinks need Developer Mode.
  download.rs    the only place HTTP happens: ureq + rustls, 300s timeout,
                 root-of-trust policy (see below)
  archive.rs     pure-Rust tar.gz/zip extraction (flate2/tar/zip crates,
                 path-traversal guarded) + `locate` (find a binary under an
                 unpredictable top-level dir) + `source_root` (unwrap forge
                 tarballs' single top dir)
  shell_init.rs  PATH wiring: idempotent marker blocks in rc files (Unix),
                 HKCU user PATH via PowerShell (Windows)
  doctor.rs      environment readout (incl. how the home was resolved)
  install/
    mod.rs       Strategy enum, auto-selection policy, shared helpers
    script.rs    fetch + run a vendor's own installer
    binary.rs    download archive -> extract -> copy binary into bin/
    source.rs    source tarball (archive_url, no git) or git clone
                 -> run build_cmd -> copy binary
```

## Command flow: `porta install <name>`

1. `manifest::load_merged()` — parse the built-in manifest (compiled into
   the binary), overlay `~/.porta/tools.toml` if present (same `name`
   replaces, new names append), validate every tool has ≥1 strategy.
2. `install::install(tool, forced)` — pick a strategy:
   - `--strategy` forces one (error if the tool doesn't declare it);
   - else `script` wins if declared (it's the vendor's blessed path);
   - else try `binary`, and on *any* failure (no target for this os-arch,
     download error, extraction error) fall back to `source` if declared.
     `source` prefers a declared `archive_url` (tarball download + built-in
     extraction — no git) and only falls back to `git clone` without one.
3. For `binary`: `version = "latest"` + `version_url` resolves the current
   version over the network (validated to look like a version string before
   it's templated into `{version}` placeholders in URLs); a pinned version
   skips that. If the target declares a `checksum`, the download's SHA-256
   is verified against the published digest — including on cache hits, so a
   corrupted or tampered cache entry can never be installed.
4. The strategy runs and returns an `Outcome { version, strategy, location }`.
5. `state.json` records the outcome (locations under `$PORTA_HOME` stored
   as `${PORTA_HOME}/...` so the file survives a machine move); PATH wiring
   re-runs (idempotent), and for `script` outcomes the vendor's install dir
   is added to PATH as well.

## PATH wiring details (`shell_init.rs`)

Unix: porta writes a block delimited by

```
# >>> porta initialize >>>
export PATH="$HOME/.porta/bin:$PATH"
# <<< porta initialize <<<
```

Paths under the home directory are deliberately written as `$HOME/...`, so
an rc file that itself travels (via `porta dotfiles`) needs no per-machine
rewrite. Re-running replaces the block in place (never duplicates it). `~/.profile`
is always updated; `.bashrc`/`.zshrc` only if they already exist or `$SHELL`
matches, so porta doesn't scatter rc files for shells you don't use. fish
gets a `fish_add_path --prepend` block in `config.fish` under the same
existence/`$SHELL` rule.

Windows: a PowerShell one-liner reads the *user* PATH via
`[Environment]::GetEnvironmentVariable('Path','User')`, prepends any missing
entries, and writes it back with `SetEnvironmentVariable(...,'User')` — the
same mechanism rustup and nvm-windows use. Machine PATH is never touched.

## TLS trust policy (`download.rs`)

All downloads use `ureq` with `rustls` — pure-Rust TLS, so porta doesn't
care whether the machine has OpenSSL, which matters on hosts you can't
install packages on.

Root-of-trust is a deliberate choice: the default is the **bundled Mozilla
root store** (webpki-roots), *not* the OS trust store. On a managed machine
the OS store may contain a TLS-inspecting proxy's root CA; porta shouldn't
silently trust that just because it's there. Users behind a *legitimate*
inspecting proxy opt in explicitly with `PORTA_TRUST_SYSTEM_CERTS=1`, which
switches to `rustls-platform-verifier` (the OS store).

## Extraction is built in — nothing on the host is assumed

`archive.rs` originally shelled out to host `tar`/`unzip`/`Expand-Archive`
on the theory that "every OS ships one". That assumption was removed: a
portable environment has to work on hosts where nothing can be counted on
(stripped containers, minimal images), and that includes porta's own
dependencies. Extraction now uses pure-Rust decompressors compiled into the
binary — `flate2` (miniz_oxide backend) + `tar` for tar.gz, `zip` (deflate
only) for zips — both of which refuse path-traversal entries. The same
principle drove the `source` strategy's `archive_url` (source tarballs
instead of requiring `git`) and the POSIX-sh bootstrap (`curl` OR `wget`,
raw release binaries needing no extractor). What remains genuinely required
is checked with an explicit error, never assumed: a build toolchain for
`source` installs, `git` only for entries without an `archive_url`.

## Error-handling conventions

- `anyhow` end to end; every fallible step wraps with `.with_context()` so a
  failure prints the *action* that failed ("downloading <url>", "writing
  <path>"), not just an OS error.
- `main` prints `porta: error: <chain>` and exits 1; no panics on expected
  failure paths.
- The binary→source fallback prints the binary failure to stderr before
  falling back, so a silent slow build never masks the original problem.

## Testing

`cargo test` covers the pieces with real logic: manifest parsing/validation
(including that the embedded manifest is valid and has the `ai` entry),
tilde expansion, version-string validation, `{version}` URL templating,
dotted-path JSON checksum lookup, SHA-256 against a FIPS test vector,
`${PORTA_HOME}` state-path round-tripping, PATH-block idempotency, quoting,
and `$HOME`-relativity, tar.gz round-trip extraction, and `locate`'s
tolerance of unpredictable archive top-level directory names, zip
round-trips, source-root detection, and rejection of a hand-crafted
path-traversal tar entry. Integration
tests (`tests/dotfiles_cli.rs`) drive the real binary in scratch
HOME/PORTA_HOME sandboxes: the dotfiles add/link/backup/remove lifecycle,
dotfiles surviving an environment move via `porta init`, and a live bash
sourcing the porta-written `.profile` and resolving a binary through the
PATH block. Install strategies are exercised end-to-end manually (`porta install ai` downloads and
checksum-verifies the real `claude` binary; a deliberately corrupted cache
entry is rejected; a copied `$PORTA_HOME` runs on a different home path)
since they are thin compositions of the tested parts plus network/process
calls.

## The bootstrap installers

`install.sh` / `install.ps1` exist because porta can't install itself. Both
follow the same shape: try to download a prebuilt `porta` release binary
(shipped raw, so no extractor is needed); if none exists, install a
**user-local** Rust toolchain via rustup (`--no-modify-path`, no admin) and
build from a **source tarball** — no git; then run `porta init --with-ai`
(skippable with `PORTA_SKIP_AI=1`). `install.sh` is POSIX `sh` (verified
under dash), works with `curl` or `wget`, and its prebuilt path needs
nothing else; `install.ps1` needs only what Windows itself ships
(PowerShell's `Invoke-WebRequest`/`Expand-Archive`). They are the only
place porta ever installs a compiler, and only for building porta itself.
