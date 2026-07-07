//! Integration tests for `porta dotfiles`, driving the real binary with a
//! scratch HOME / PORTA_HOME per invocation (env vars are per child
//! process, so nothing here races the unit tests).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

struct Env {
    home: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let home = std::env::temp_dir().join(format!("porta-itest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        Env { home }
    }

    fn porta(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_porta"))
            .args(args)
            .env("HOME", &self.home)
            .env("PORTA_HOME", self.home.join(".porta"))
            .env("SHELL", "/bin/bash")
            .output()
            .expect("failed to run porta binary")
    }

    fn porta_ok(&self, args: &[&str]) -> String {
        let out = self.porta(args);
        assert!(
            out.status.success(),
            "porta {args:?} failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.is_symlink())
        .unwrap_or(false)
}

#[test]
fn dotfiles_add_link_backup_remove_lifecycle() {
    let env = Env::new("lifecycle");
    let rc = env.home.join(".testrc");
    std::fs::write(&rc, "alias ll='ls -l'\n").unwrap();

    // add: file moves into the store, a symlink replaces it
    env.porta_ok(&["dotfiles", "add", rc.to_str().unwrap()]);
    assert!(is_symlink(&rc));
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), "alias ll='ls -l'\n");
    let stored = env.home.join(".porta/dotfiles/.testrc");
    assert!(stored.is_file());

    // list shows it linked
    let listing = env.porta_ok(&["dotfiles", "list"]);
    assert!(listing.contains(".testrc"), "{listing}");
    assert!(listing.contains("linked"), "{listing}");

    // simulate arriving on a machine that already has its own .testrc
    std::fs::remove_file(&rc).unwrap();
    std::fs::write(&rc, "machine-local content\n").unwrap();
    env.porta_ok(&["dotfiles", "link"]);
    assert!(is_symlink(&rc));
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), "alias ll='ls -l'\n");
    assert_eq!(
        std::fs::read_to_string(env.home.join(".testrc.porta-backup")).unwrap(),
        "machine-local content\n"
    );

    // link is idempotent
    env.porta_ok(&["dotfiles", "link"]);

    // remove: a real file is restored, store entry and tracking are gone
    env.porta_ok(&["dotfiles", "remove", ".testrc"]);
    assert!(!is_symlink(&rc));
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), "alias ll='ls -l'\n");
    assert!(!stored.exists());
    let listing = env.porta_ok(&["dotfiles", "list"]);
    assert!(listing.contains("no dotfiles tracked"), "{listing}");
}

#[test]
fn dotfiles_survive_environment_move_via_init() {
    let machine_a = Env::new("move-a");
    let gitconfig = machine_a.home.join(".gitconfig");
    std::fs::write(&gitconfig, "[user]\n\tname = Porta Tester\n").unwrap();
    machine_a.porta_ok(&["dotfiles", "add", gitconfig.to_str().unwrap()]);

    // "move" ~/.porta to a machine with a different home path
    let machine_b = Env::new("move-b");
    copy_dir(
        &machine_a.home.join(".porta"),
        &machine_b.home.join(".porta"),
    );

    // porta init on the new machine re-links tracked dotfiles
    let out = machine_b.porta_ok(&["init"]);
    assert!(
        out.contains("dotfile"),
        "init should report dotfile linking: {out}"
    );

    let moved = machine_b.home.join(".gitconfig");
    assert!(is_symlink(&moved));
    assert_eq!(
        std::fs::read_to_string(&moved).unwrap(),
        "[user]\n\tname = Porta Tester\n"
    );
}

#[test]
fn path_block_actually_works_in_bash() {
    let env = Env::new("pathblock");
    env.porta_ok(&["init"]);

    // Drop a fake tool into porta's bin and prove a login-ish bash started
    // with the porta-written .profile can find it via PATH.
    let bin = env.home.join(".porta/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tool = bin.join("porta-canary");
    std::fs::write(&tool, "#!/bin/sh\necho canary-ok\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();

    let out = Command::new("bash")
        .args(["-c", ". ~/.profile && porta-canary"])
        .env("HOME", &env.home)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "canary-ok");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        let meta = entry.metadata().unwrap();
        if meta.is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}
