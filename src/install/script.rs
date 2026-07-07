//! The `script` strategy: download and run a tool's own official installer.
//!
//! This exists for tools (Claude Code chief among them) that already ship a
//! trustworthy, portable, no-admin installer of their own — reimplementing
//! that logic in porta would just be a second place for it to go stale.
//! porta's job here is narrow: fetch the script over HTTPS, run it with the
//! interpreter the vendor expects, and make sure the directory it installs
//! into ends up on `PATH`.

use crate::install::{Outcome, Strategy};
use crate::manifest::{ScriptSpec, ScriptTarget, Tool};
use anyhow::{bail, Context, Result};
use std::process::Command;

pub fn install(tool: &Tool, spec: &ScriptSpec) -> Result<Outcome> {
    let target = if cfg!(windows) {
        spec.windows.as_ref()
    } else {
        spec.unix.as_ref()
    };
    let Some(target) = target else {
        bail!(
            "`{}` has no installer for this platform ({})",
            tool.name,
            std::env::consts::OS
        );
    };

    println!(
        "porta: installing `{}` via its official installer ({})",
        tool.label(),
        target.url
    );

    run_installer(target).with_context(|| format!("running installer for `{}`", tool.name))?;

    let install_dir = crate::manifest::expand_tilde(&spec.installs_to);
    Ok(Outcome {
        version: "vendor-managed".to_string(),
        strategy: Strategy::Script,
        location: install_dir.display().to_string(),
    })
}

fn run_installer(target: &ScriptTarget) -> Result<()> {
    let script_text = crate::download::fetch_text(&target.url)?;

    crate::paths::ensure_layout()?;
    let scratch = crate::paths::cache_dir().join("installer-scripts");
    std::fs::create_dir_all(&scratch)?;
    let script_path = scratch.join(script_file_name(&target.url, &target.interpreter));
    std::fs::write(&script_path, &script_text)
        .with_context(|| format!("writing {}", script_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path)?.permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(&script_path, perms)?;
    }

    // The manifest's `args` may contain a literal `{script}` placeholder to
    // control exactly where the script path lands (PowerShell needs
    // `-File <path>` in a specific position); if it doesn't, the path is
    // simply appended, which is correct for a plain `bash <path>` call.
    let script_path_str = script_path.display().to_string();
    let cmd_args: Vec<String> = if target.args.iter().any(|a| a == "{script}") {
        target
            .args
            .iter()
            .map(|a| {
                if a == "{script}" {
                    script_path_str.clone()
                } else {
                    a.clone()
                }
            })
            .collect()
    } else {
        let mut args = target.args.clone();
        args.push(script_path_str);
        args
    };

    let mut cmd = Command::new(&target.interpreter);
    cmd.args(&cmd_args);

    let status = cmd
        .status()
        .with_context(|| format!("failed to launch `{}`", target.interpreter))?;
    if !status.success() {
        bail!("installer exited with {status}");
    }
    Ok(())
}

fn script_file_name(url: &str, interpreter: &str) -> String {
    let ext = match interpreter {
        "powershell" | "pwsh" => "ps1",
        _ => "sh",
    };
    let stem = url
        .rsplit('/')
        .next()
        .unwrap_or("installer")
        .split('?')
        .next()
        .unwrap_or("installer");
    if stem.contains('.') {
        stem.to_string()
    } else {
        format!("{stem}.{ext}")
    }
}
