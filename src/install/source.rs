//! The `source` strategy: `git clone` a tool's repository and build it
//! locally with whatever command the manifest specifies (typically
//! `cargo build --release`). Requires the relevant toolchain to already be
//! on `PATH` — porta doesn't install compilers itself, only what it builds
//! with them.

use crate::install::{binary_file_name, make_executable, Outcome, Strategy};
use crate::manifest::{SourceSpec, Tool};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub fn install(tool: &Tool, spec: &SourceSpec) -> Result<Outcome> {
    let build_tool = spec
        .build_cmd
        .first()
        .context("`build_cmd` in manifest must have at least one element")?;
    which(build_tool).with_context(|| {
        format!(
            "`{build_tool}` is not on PATH — install it before building `{}` from source",
            tool.name
        )
    })?;

    let checkout_dir = crate::paths::tools_dir().join(format!("{}-src", tool.name));
    if checkout_dir.exists() {
        std::fs::remove_dir_all(&checkout_dir)
            .with_context(|| format!("clearing stale checkout at {}", checkout_dir.display()))?;
    }

    println!(
        "porta: cloning `{}` from {} to build from source",
        tool.label(),
        spec.repo
    );
    clone(spec, &checkout_dir)?;

    println!(
        "porta: building `{}` ({})",
        tool.label(),
        spec.build_cmd.join(" ")
    );
    run_build(&spec.build_cmd, &checkout_dir)?;

    let built = checkout_dir.join(&spec.binary_path);
    if !built.exists() {
        bail!(
            "build succeeded but expected binary was not found at {}",
            built.display()
        );
    }

    let dest_bin = crate::paths::bin_dir().join(binary_file_name(tool.bin_name()));
    std::fs::create_dir_all(crate::paths::bin_dir())?;
    std::fs::copy(&built, &dest_bin)
        .with_context(|| format!("copying {} to {}", built.display(), dest_bin.display()))?;
    make_executable(&dest_bin)?;

    let version = spec.git_ref.clone().unwrap_or_else(|| "source".to_string());
    Ok(Outcome {
        version,
        strategy: Strategy::Source,
        location: dest_bin.display().to_string(),
    })
}

fn clone(spec: &SourceSpec, dest: &Path) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(git_ref) = &spec.git_ref {
        cmd.arg("--branch").arg(git_ref);
    }
    cmd.arg(&spec.repo).arg(dest);

    let status = cmd.status().context("failed to launch `git`")?;
    if !status.success() {
        bail!("`git clone {}` exited with {status}", spec.repo);
    }
    Ok(())
}

fn run_build(build_cmd: &[String], dir: &Path) -> Result<()> {
    let (program, args) = build_cmd
        .split_first()
        .context("`build_cmd` in manifest must have at least one element")?;
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("failed to launch `{program}`"))?;
    if !status.success() {
        bail!("`{}` exited with {status}", build_cmd.join(" "));
    }
    Ok(())
}

fn which(program: &str) -> Result<()> {
    let found = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(program);
                candidate.is_file()
                    || (cfg!(windows) && dir.join(format!("{program}.exe")).is_file())
            })
        })
        .unwrap_or(false);
    if found {
        Ok(())
    } else {
        bail!("not found on PATH")
    }
}
