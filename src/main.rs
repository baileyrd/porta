mod archive;
mod cli;
mod doctor;
mod download;
mod install;
mod manifest;
mod paths;
mod shell_init;
mod state;

use anyhow::{bail, Context, Result};
use clap::Parser;
use cli::{Cli, Command};
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
        Command::Init { with_ai } => cmd_init(with_ai),
        Command::Doctor => doctor::run(),
        Command::List => cmd_list(),
        Command::Install { name, strategy } => cmd_install(&name, strategy.as_deref()),
        Command::Uninstall { name } => cmd_uninstall(&name),
        Command::Path => cmd_path(),
        Command::Which { name } => cmd_which(&name),
    }
}

fn cmd_init(with_ai: bool) -> Result<()> {
    paths::ensure_layout().context("creating porta's directories")?;
    println!("porta home: {}", paths::porta_home().display());

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

    if with_ai {
        cmd_install("ai", None)?;
    } else {
        println!("\nNext: `porta install ai` to set up the bundled AI coding CLI.");
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
