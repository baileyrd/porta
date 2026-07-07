mod archive;
mod cli;
mod doctor;
mod dotfiles;
mod download;
mod install;
mod manifest;
mod paths;
mod shell_init;
mod state;

use anyhow::{bail, Context, Result};
use clap::Parser;
use cli::{Cli, Command, DotfilesAction};
use install::Strategy;
use state::State;

fn main() {
    if let Err(err) = run() {
        eprintln!("porta: error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { home, with_ai } => cmd_init(home, with_ai),
        Command::Move { new_home } => cmd_move(&new_home),
        Command::Doctor => doctor::run(),
        Command::List => cmd_list(),
        Command::Install { name, strategy } => cmd_install(&name, strategy.as_deref()),
        Command::Uninstall { name } => cmd_uninstall(&name),
        Command::Path => cmd_path(),
        Command::Which { name } => cmd_which(&name),
        Command::Dotfiles { action } => match action {
            DotfilesAction::Add { paths } => dotfiles::add(&paths),
            DotfilesAction::Link => {
                let n = dotfiles::link_all()?;
                println!("porta: {n} dotfile(s) linked");
                Ok(())
            }
            DotfilesAction::List => dotfiles::list(),
            DotfilesAction::Remove { path } => dotfiles::remove(&path),
        },
    }
}

fn cmd_init(home: Option<std::path::PathBuf>, with_ai: bool) -> Result<()> {
    if let Some(home) = home {
        let home =
            std::path::absolute(&home).with_context(|| format!("resolving {}", home.display()))?;
        // Steer every downstream porta_home() call in this process at the
        // chosen directory. Future invocations won't need this: the binary
        // installed into <home>/bin self-locates via <home>/state.json.
        std::env::set_var("PORTA_HOME", &home);
        println!("porta home designated at {}", home.display());
    }

    paths::ensure_layout().context("creating porta's directories")?;
    println!("porta home: {}", paths::porta_home().display());

    install_self_into_bin()?;

    // Write state.json (even if empty): it is the marker that lets the
    // binary in <home>/bin find this directory as its home from now on,
    // with no environment variable set.
    State::load()?.save()?;

    let touched = shell_init::wire_path(&[paths::bin_dir()])?;
    if touched.is_empty() {
        println!("PATH already configured.");
    } else {
        println!("Added {} to PATH via:", paths::bin_dir().display());
        for t in &touched {
            println!("  {t}");
        }
        println!("Restart your shell (or run `porta path` to apply it now) to pick this up.");
    }

    // Re-link tracked dotfiles — this is what makes `copy ~/.porta; porta
    // init` bring your configuration along on a new machine.
    let linked = dotfiles::link_all()?;
    if linked > 0 {
        println!("Linked {linked} tracked dotfile(s) into your home directory.");
    }

    if with_ai {
        cmd_install("ai", None)?;
    } else {
        println!("\nNext: `porta install ai` to set up the bundled AI coding CLI.");
    }

    Ok(())
}

/// Copy the running porta executable into `<home>/bin` when it isn't
/// already there, so the environment carries its own porta and the binary
/// can self-locate its home.
fn install_self_into_bin() -> Result<()> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .context("locating the running porta executable")?;
    let dest = paths::bin_dir().join(format!("porta{}", std::env::consts::EXE_SUFFIX));

    if let Ok(existing) = dest.canonicalize() {
        if existing == exe {
            return Ok(()); // already running from <home>/bin
        }
    }

    std::fs::create_dir_all(paths::bin_dir())?;
    std::fs::copy(&exe, &dest)
        .with_context(|| format!("copying {} to {}", exe.display(), dest.display()))?;
    install::make_executable(&dest)?;
    println!("Installed the porta binary at {}", dest.display());
    Ok(())
}

fn cmd_move(new_home: &std::path::Path) -> Result<()> {
    let env_was_set = std::env::var_os("PORTA_HOME").is_some();
    let old_home = paths::porta_home();
    let new_home = std::path::absolute(new_home)
        .with_context(|| format!("resolving {}", new_home.display()))?;

    if !old_home.is_dir() {
        bail!(
            "current porta home {} does not exist — nothing to move",
            old_home.display()
        );
    }
    if new_home == old_home {
        bail!("{} is already the porta home", new_home.display());
    }
    if new_home.starts_with(&old_home) || old_home.starts_with(&new_home) {
        bail!(
            "cannot move {} into {} — one contains the other",
            old_home.display(),
            new_home.display()
        );
    }
    if new_home.exists() {
        let occupied = std::fs::read_dir(&new_home)
            .with_context(|| format!("reading {}", new_home.display()))?
            .next()
            .is_some();
        if occupied {
            bail!(
                "{} already exists and is not empty — refusing to move into it",
                new_home.display()
            );
        }
        std::fs::remove_dir(&new_home).ok();
    }

    // Script-strategy tools live outside the home; their PATH entries must
    // survive the block rewrite below.
    let state = State::load()?;
    let script_dirs: Vec<std::path::PathBuf> = state
        .tools
        .values()
        .filter(|t| t.strategy == "script")
        .map(|t| std::path::PathBuf::from(t.resolved_location()))
        .collect();

    println!(
        "porta: moving {} -> {}",
        old_home.display(),
        new_home.display()
    );
    if let Some(parent) = new_home.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    if std::fs::rename(&old_home, &new_home).is_err() {
        // Different filesystem/drive (e.g. ~ on C: -> D:\tools\porta):
        // copy, then best-effort delete. On Windows the running porta.exe
        // inside the old home can't delete itself — say so instead of
        // failing the move.
        dotfiles::copy_recursively(&old_home, &new_home)?;
        if let Err(err) = std::fs::remove_dir_all(&old_home) {
            eprintln!(
                "porta: note: moved by copy, but couldn't fully remove the old \
                 directory at {} ({err}); delete it manually once this porta \
                 process has exited",
                old_home.display()
            );
        }
    }

    // Re-point this process at the new home for the wiring below. Future
    // invocations of <new_home>/bin/porta self-locate via state.json.
    std::env::set_var("PORTA_HOME", &new_home);

    shell_init::remove_user_path_entry(&old_home.join("bin"))?;
    let mut path_dirs = vec![paths::bin_dir()];
    path_dirs.extend(script_dirs);
    shell_init::wire_path(&path_dirs)?;

    let relinked = dotfiles::link_all()?;
    if relinked > 0 {
        println!("Re-linked {relinked} tracked dotfile(s) to the new location.");
    }

    println!(
        "porta: environment moved to {}. Restart your shell to pick up the new PATH; \
         from now on run {}",
        new_home.display(),
        paths::bin_dir()
            .join(format!("porta{}", std::env::consts::EXE_SUFFIX))
            .display()
    );
    if env_was_set {
        println!(
            "note: $PORTA_HOME is set in your environment and still points at the old \
             location — update it to {} or unset it (the moved binary finds its home \
             by itself).",
            new_home.display()
        );
    }
    Ok(())
}

fn cmd_list() -> Result<()> {
    let manifest = manifest::load_merged()?;
    let state = State::load()?;

    for tool in &manifest.tools {
        let installed = state.tools.get(&tool.name);
        let mut strategies = Vec::new();
        if tool.script.is_some() {
            strategies.push("script");
        }
        if tool.binary.is_some() {
            strategies.push("binary");
        }
        if tool.source.is_some() {
            strategies.push("source");
        }

        let status = match installed {
            Some(t) => format!("installed ({} via {})", t.version, t.strategy),
            None => "not installed".to_string(),
        };

        println!("{:<12} [{}] - {}", tool.name, strategies.join("/"), status);
        if let Some(desc) = &tool.description {
            println!("             {desc}");
        }
    }

    Ok(())
}

fn cmd_install(name: &str, strategy: Option<&str>) -> Result<()> {
    let manifest = manifest::load_merged()?;
    let tool = manifest
        .find(name)
        .with_context(|| format!("no tool named `{name}` in the manifest (see `porta list`)"))?;

    let forced = strategy.map(Strategy::parse).transpose()?;
    let outcome = install::install(tool, forced)?;

    let mut state = State::load()?;
    state.record(
        &tool.name,
        &outcome.version,
        outcome.strategy.as_str(),
        &outcome.location,
    );
    state.save()?;

    let mut path_dirs = vec![paths::bin_dir()];
    if outcome.strategy == Strategy::Script {
        path_dirs.push(std::path::PathBuf::from(&outcome.location));
    }
    shell_init::wire_path(&path_dirs)?;

    println!(
        "porta: installed `{}` ({}) via {} -> {}",
        tool.label(),
        outcome.version,
        outcome.strategy.as_str(),
        outcome.location
    );
    Ok(())
}

fn cmd_uninstall(name: &str) -> Result<()> {
    let mut state = State::load()?;
    let Some(installed) = state.remove(name) else {
        bail!("`{name}` is not tracked as installed by porta");
    };

    let location = installed.resolved_location();

    if installed.strategy == "script" {
        state.save()?;
        println!(
            "porta: forgot about `{name}`, but did not remove its own installation at {location} \
             (it manages its own uninstall — see the tool's docs)."
        );
        return Ok(());
    }

    let path = std::path::Path::new(&location);
    if path.exists() {
        std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
    }
    state.save()?;
    println!("porta: removed `{name}` ({location})");
    Ok(())
}

fn cmd_path() -> Result<()> {
    let bin = paths::bin_dir();
    if cfg!(windows) {
        println!(
            "porta's bin dir is: {}\n\
             `porta init` already adds this to your user PATH (HKCU\\Environment).\n\
             Open a new terminal to pick it up.",
            bin.display()
        );
    } else {
        println!("export PATH=\"{}:$PATH\"", bin.display());
    }
    Ok(())
}

fn cmd_which(name: &str) -> Result<()> {
    let state = State::load()?;
    let installed = state
        .tools
        .get(name)
        .with_context(|| format!("`{name}` is not installed"))?;
    println!("{}", installed.resolved_location());
    Ok(())
}
