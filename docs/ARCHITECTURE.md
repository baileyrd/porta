# porta architecture

porta is a small Rust CLI (~1,200 lines across nine modules) with one job:
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
  state.json      <- registry of what porta installed (version/strategy/where)
  tools.toml      <- OPTIONAL user manifest, merged over the built-in one
```

`script`-strategy tools are the deliberate exception to `bin/`: the vendor's
installer owns its install location (Claude Code uses `~/.local/bin` and its
own background updater), so porta records that directory in `state.json` and
adds it to PATH rather than copying the binary — copying would break the
vendor's update mechanism.

## Module map

```
src/
  main.rs        clap dispatch; one cmd_* fn per subcommand
  cli.rs         clap derive types (the CLI surface, nothing else)
  paths.rs       every filesystem location, in one place; PORTA_HOME override
  manifest.rs    TOML schema (Tool/ScriptSpec/BinarySpec/SourceSpec),
                 built-in manifest embedded via include_str!, user-manifest
                 merge (by tool name), target-key resolution (os-arch)
  state.rs       state.json load/save; record/remove entries
  download.rs    the only place HTTP happens: ureq + rustls, 300s timeout,
                 root-of-trust policy (see below)
  archive.rs     extraction by shelling out (tar / unzip / Expand-Archive)
                 + `locate`, which finds a binary inside an archive whose
                 top-level directory name wasn't predictable
  shell_init.rs  PATH wiring: idempotent marker blocks in rc files (Unix),
                 HKCU user PATH via PowerShell (Windows)
  doctor.rs      environment readout
  install/
    mod.rs       Strategy enum, auto-selection policy, shared helpers
    script.rs    fetch + run a vendor's own installer
    binary.rs    download archive -> extract -> copy binary into bin/
    source.rs    git clone --depth 1 -> run build_cmd -> copy binary
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
3. The strategy runs and returns an `Outcome { version, strategy, location }`.
4. `state.json` records the outcome; PATH wiring re-runs (idempotent), and
   for `script` outcomes the vendor's install dir is added to PATH as well.

## PATH wiring details (`shell_init.rs`)

Unix: porta writes a block delimited by

```
# >>> porta initialize >>>
export PATH="'/home/you/.porta/bin':$PATH"
# <<< porta initialize <<<
```

Re-running replaces the block in place (never duplicates it). `~/.profile`
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

## Why extraction shells out

`archive.rs` runs `tar -xzf` / `tar -xf`, falling back to `unzip` (Unix) or
`Expand-Archive` (Windows) for zips. Vendoring a decompressor was rejected:
every target OS ships `tar` (Windows 10 1803+ includes bsdtar), the shell-out
needs no privileges, and it keeps porta's dependency tree small enough to
audit. The trade-off is a runtime dependency on those tools existing, which
`porta doctor`-style errors surface clearly when violated.

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
tilde expansion, PATH-block idempotency and content preservation, tar.gz
round-trip extraction, and `locate`'s tolerance of unpredictable archive
top-level directory names. Install strategies are exercised end-to-end
manually (`porta install ai` runs the real Claude Code installer) since they
are thin compositions of the tested parts plus network/process calls.

## The bootstrap installers

`install.sh` / `install.ps1` exist because porta can't install itself. Both
follow the same shape: try to download a prebuilt `porta` release for the
platform; if none exists, install a **user-local** Rust toolchain via rustup
(`--no-modify-path`, no admin) and build from a shallow clone; then run
`porta init --with-ai` (skippable with `PORTA_SKIP_AI=1`). They are the only
place porta ever installs a compiler, and only for building porta itself.
