//! Integration tests for designating and moving the porta home:
//! `porta init --home <dir>`, executable self-location (no env var), and
//! `porta move <dir>`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

struct Scratch {
    home: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let home =
            std::env::temp_dir().join(format!("porta-home-itest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        // Canonicalize: macOS's temp dir lives under /var -> /private/var,
        // and the self-location assertions compare against doctor output,
        // which reports the canonicalized (exe-derived) home path.
        let home = home.canonicalize().unwrap();
        Scratch { home }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// Run a porta binary with a controlled HOME and an *optional* PORTA_HOME —
/// the self-location tests depend on the env var being absent.
fn run(bin: &Path, args: &[&str], home: &Path, porta_home: Option<&Path>) -> String {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("HOME", home)
        .env("SHELL", "/bin/sh")
        .env_remove("PORTA_HOME");
    if let Some(ph) = porta_home {
        cmd.env("PORTA_HOME", ph);
    }
    let out = cmd.output().expect("failed to run porta");
    assert!(
        out.status.success(),
        "porta {args:?} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn init_home_designates_custom_location_and_binary_self_locates() {
    let scratch = Scratch::new("designate");
    let custom = scratch.home.join("tools/porta");

    // porta init --home <custom>, no PORTA_HOME env anywhere.
    run(
        Path::new(env!("CARGO_BIN_EXE_porta")),
        &["init", "--home", custom.to_str().unwrap()],
        &scratch.home,
        None,
    );

    // Layout + the marker + a copy of the binary live at the custom path.
    assert!(custom.join("bin/porta").is_file(), "binary copied inside");
    assert!(custom.join("state.json").is_file(), "marker written");
    assert!(custom.join("cache").is_dir());

    // The PATH block points at the custom location ($HOME-relative since
    // it's under the test HOME).
    let profile = std::fs::read_to_string(scratch.home.join(".profile")).unwrap();
    assert!(
        profile.contains("$HOME/tools/porta/bin"),
        "profile block should reference the custom bin: {profile}"
    );

    // The installed copy finds its home purely from its own location.
    let out = run(&custom.join("bin/porta"), &["doctor"], &scratch.home, None);
    assert!(
        out.contains(&format!(
            "porta home:   {} (located from the porta executable)",
            custom.display()
        )),
        "doctor should self-locate at the custom home: {out}"
    );
}

#[test]
fn move_relocates_environment_and_relinks() {
    let scratch = Scratch::new("move");
    let old_home = scratch.home.join(".porta");
    let new_home = scratch.home.join("relocated/porta");

    // Set up at the default-style location, with a tracked dotfile.
    run(
        Path::new(env!("CARGO_BIN_EXE_porta")),
        &["init"],
        &scratch.home,
        Some(&old_home),
    );
    let rc = scratch.home.join(".testrc");
    std::fs::write(&rc, "export TEST=1\n").unwrap();
    run(
        Path::new(env!("CARGO_BIN_EXE_porta")),
        &["dotfiles", "add", rc.to_str().unwrap()],
        &scratch.home,
        Some(&old_home),
    );

    // Move it.
    let out = run(
        Path::new(env!("CARGO_BIN_EXE_porta")),
        &["move", new_home.to_str().unwrap()],
        &scratch.home,
        Some(&old_home),
    );
    assert!(out.contains("environment moved"), "{out}");
    // The env var was set for this invocation, so the stale-var warning
    // must appear.
    assert!(out.contains("$PORTA_HOME is set"), "{out}");

    assert!(!old_home.exists(), "old home should be gone");
    assert!(new_home.join("state.json").is_file());
    assert!(new_home.join("bin/porta").is_file());

    // PATH block rewritten to the new location.
    let profile = std::fs::read_to_string(scratch.home.join(".profile")).unwrap();
    assert!(
        profile.contains("$HOME/relocated/porta/bin"),
        "profile should point at the new bin: {profile}"
    );
    assert!(
        !profile.contains("$HOME/.porta/bin"),
        "old bin must not linger in the block: {profile}"
    );

    // Dotfile symlink re-pointed into the moved store, content intact.
    let target = std::fs::read_link(&rc).unwrap();
    assert_eq!(target, new_home.join("dotfiles/.testrc"));
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), "export TEST=1\n");

    // The moved binary self-locates with no env var at all.
    let out = run(
        &new_home.join("bin/porta"),
        &["doctor"],
        &scratch.home,
        None,
    );
    assert!(
        out.contains(&format!(
            "porta home:   {} (located from the porta executable)",
            new_home.display()
        )),
        "moved binary should self-locate: {out}"
    );
}

#[test]
fn move_refuses_nested_and_nonempty_targets() {
    let scratch = Scratch::new("move-guard");
    let old_home = scratch.home.join(".porta");
    run(
        Path::new(env!("CARGO_BIN_EXE_porta")),
        &["init"],
        &scratch.home,
        Some(&old_home),
    );

    let refuse = |target: &Path| {
        let out = Command::new(env!("CARGO_BIN_EXE_porta"))
            .args(["move", target.to_str().unwrap()])
            .env("HOME", &scratch.home)
            .env("SHELL", "/bin/sh")
            .env("PORTA_HOME", &old_home)
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "move to {} should fail",
            target.display()
        );
    };

    // Into itself / into a subdirectory of itself.
    refuse(&old_home);
    refuse(&old_home.join("inside"));

    // Into an existing non-empty directory.
    let occupied = scratch.home.join("occupied");
    std::fs::create_dir_all(occupied.join("stuff")).unwrap();
    refuse(&occupied);

    // The environment is untouched after refusals.
    assert!(old_home.join("state.json").is_file());
}
