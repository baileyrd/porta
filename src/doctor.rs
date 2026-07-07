//! `porta doctor` — a quick, human-readable readout of environment health:
//! where porta's home is, whether it's actually on `PATH`, and what's
//! installed.

use crate::state::State;
use anyhow::Result;

pub fn run() -> Result<()> {
    let home = crate::paths::porta_home();
    println!("porta home:   {}", home.display());
    println!("bin dir:      {}", crate::paths::bin_dir().display());

    let bin_dir = crate::paths::bin_dir();
    let on_path = path_contains(&bin_dir);
    println!(
        "on PATH:      {}",
        if on_path {
            "yes"
        } else {
            "no — run `porta init`, then restart your shell"
        }
    );

    let state = State::load()?;
    if state.tools.is_empty() {
        println!("\ninstalled:    (nothing yet — try `porta install ai`)");
    } else {
        println!("\ninstalled:");
        let mut names: Vec<&String> = state.tools.keys().collect();
        names.sort();
        for name in names {
            let tool = &state.tools[name];
            println!(
                "  {name:<12} {:<10} via {:<8} -> {}",
                tool.version,
                tool.strategy,
                tool.resolved_location()
            );
        }
    }

    if which("claude") {
        println!("\nclaude:       found on PATH");
    } else if state.tools.contains_key("ai") {
        println!("\nclaude:       installed but not yet on PATH in this shell (restart it)");
    }

    Ok(())
}

fn path_contains(dir: &std::path::Path) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|p| paths_equal(&p, dir)))
        .unwrap_or(false)
}

fn paths_equal(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                dir.join(program).is_file()
                    || (cfg!(windows) && dir.join(format!("{program}.exe")).is_file())
            })
        })
        .unwrap_or(false)
}
