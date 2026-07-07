//! Wires porta's directories onto `PATH` — the one persistent, user-visible
//! side effect `porta init` has. Everything here is scoped to the current
//! user: rc files under `$HOME` on Unix, the `HKCU` (`User`) environment
//! block on Windows. Nothing here ever touches a system-wide location or
//! needs elevated privileges.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const BEGIN_MARKER: &str = "# >>> porta initialize >>>";
const END_MARKER: &str = "# <<< porta initialize <<<";

/// Adds `dirs` to `PATH` for the current user, returning the list of files
/// (Unix) or `"Windows user environment"` (Windows) it touched.
pub fn wire_path(dirs: &[PathBuf]) -> Result<Vec<String>> {
    if dirs.is_empty() {
        return Ok(Vec::new());
    }
    if cfg!(windows) {
        wire_windows(dirs)
    } else {
        wire_unix(dirs)
    }
}

fn wire_unix(dirs: &[PathBuf]) -> Result<Vec<String>> {
    let home = crate::paths::home_dir();
    let mut touched = Vec::new();

    let posix_block = posix_export_block(dirs);
    for rc in candidate_posix_rc_files(&home) {
        write_block(&rc, &posix_block)?;
        touched.push(rc.display().to_string());
    }

    let fish_config = home.join(".config/fish/config.fish");
    if fish_config.exists() || is_current_shell("fish") {
        let block = fish_export_block(dirs);
        write_block(&fish_config, &block)?;
        touched.push(fish_config.display().to_string());
    }

    Ok(touched)
}

/// Always updates `~/.profile` (read by most login shells); for the other
/// well-known rc files, only touches ones that already exist or match
/// `$SHELL`, so porta doesn't scatter rc files for shells the user doesn't
/// use.
fn candidate_posix_rc_files(home: &Path) -> Vec<PathBuf> {
    let mut files = vec![home.join(".profile")];

    let bashrc = home.join(".bashrc");
    if bashrc.exists() || is_current_shell("bash") {
        files.push(bashrc);
    }
    let zshrc = home.join(".zshrc");
    if zshrc.exists() || is_current_shell("zsh") {
        files.push(zshrc);
    }

    files
}

fn is_current_shell(name: &str) -> bool {
    std::env::var("SHELL")
        .map(|shell| shell.contains(name))
        .unwrap_or(false)
}

fn posix_export_block(dirs: &[PathBuf]) -> String {
    let prepend = dirs
        .iter()
        .map(|d| double_quoted_path(d))
        .collect::<Vec<_>>()
        .join(":");
    format!("export PATH=\"{prepend}:$PATH\"")
}

fn fish_export_block(dirs: &[PathBuf]) -> String {
    let prepend = dirs
        .iter()
        .map(|d| format!("\"{}\"", double_quoted_path(d)))
        .collect::<Vec<_>>()
        .join(" ");
    format!("fish_add_path --prepend {prepend}")
}

/// Render a path for use inside a double-quoted shell string. Directories
/// under the user's home are written as `$HOME/...` so the rc block itself
/// stays correct when the whole environment (and the rc file with it) is
/// copied to a machine with a different home path.
fn double_quoted_path(dir: &Path) -> String {
    let home = crate::paths::home_dir();
    match dir.strip_prefix(&home) {
        Ok(rest) => format!(
            "$HOME/{}",
            escape_in_double_quotes(&rest.display().to_string())
        ),
        Err(_) => escape_in_double_quotes(&dir.display().to_string()),
    }
}

/// Escape the characters that are special inside POSIX/fish double quotes.
fn escape_in_double_quotes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Writes `body` between porta's markers in `path`, replacing a previous
/// block if one is already there so re-running `porta init` is idempotent.
fn write_block(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let block = format!("{BEGIN_MARKER}\n{body}\n{END_MARKER}");

    let new_contents = if let Some(replaced) = replace_marked_block(&existing, &block) {
        replaced
    } else {
        let mut updated = existing;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&block);
        updated.push('\n');
        updated
    };

    std::fs::write(path, new_contents).with_context(|| format!("writing {}", path.display()))
}

fn replace_marked_block(existing: &str, new_block: &str) -> Option<String> {
    let start = existing.find(BEGIN_MARKER)?;
    let end_marker_pos = existing[start..].find(END_MARKER)?;
    let end = start + end_marker_pos + END_MARKER.len();

    let mut result = String::new();
    result.push_str(&existing[..start]);
    result.push_str(new_block);
    result.push_str(&existing[end..]);
    Some(result)
}

/// Remove a directory from the user-scope PATH. Used by `porta move` so
/// the old bin directory doesn't linger. On Unix this is a no-op: the rc
/// marker block is rewritten wholesale, which drops old entries by itself.
pub fn remove_user_path_entry(dir: &Path) -> Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    let target = dir.display().to_string();
    let script = format!(
        "$old = [Environment]::GetEnvironmentVariable('Path', 'User');\n\
         if ($old) {{\n\
           $parts = $old.Split(';') | Where-Object {{ $_.TrimEnd('\\') -ne '{target}'.TrimEnd('\\') }};\n\
           [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User');\n\
         }}\n"
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .context("failed to launch `powershell` to update the user PATH")?;
    if !status.success() {
        anyhow::bail!("powershell exited with {status} while removing a PATH entry");
    }
    Ok(())
}

fn wire_windows(dirs: &[PathBuf]) -> Result<Vec<String>> {
    // User-scope (`HKCU\Environment`) PATH update via .NET's Environment
    // API, invoked through PowerShell — the same mechanism rustup, nvm-
    // windows, and friends use. This never touches the machine-scope PATH
    // and never requires an elevated (Administrator) prompt.
    let joined = dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(";");

    let script = format!(
        "$new = @('{joined}');\n\
         $old = [Environment]::GetEnvironmentVariable('Path', 'User');\n\
         $parts = @();\n\
         if ($old) {{ $parts = $old.Split(';') }};\n\
         foreach ($p in $new) {{ if ($parts -notcontains $p) {{ $parts = @($p) + $parts }} }};\n\
         [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User');\n"
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .context("failed to launch `powershell` to update the user PATH")?;
    if !status.success() {
        anyhow::bail!("powershell exited with {status} while updating PATH");
    }

    Ok(vec![
        "Windows user environment (HKCU\\Environment)".to_string()
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_block_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("porta-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rc");

        write_block(&file, "export PATH=\"/a:$PATH\"").unwrap();
        write_block(&file, "export PATH=\"/b:$PATH\"").unwrap();

        let contents = std::fs::read_to_string(&file).unwrap();
        assert_eq!(contents.matches(BEGIN_MARKER).count(), 1);
        assert!(contents.contains("/b:$PATH"));
        assert!(!contents.contains("/a:$PATH"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn path_blocks_are_home_relative_and_unquoted() {
        let home = crate::paths::home_dir();
        let under_home = home.join(".porta/bin");
        let block = posix_export_block(std::slice::from_ref(&under_home));

        // The literal-apostrophe bug: quotes nested inside the double-quoted
        // export made PATH entries that contained `'` characters.
        assert!(!block.contains('\''), "no single quotes in: {block}");
        assert_eq!(block, "export PATH=\"$HOME/.porta/bin:$PATH\"");

        // Outside home: absolute, still unquoted.
        let outside = PathBuf::from("/opt/porta/bin");
        assert_eq!(
            posix_export_block(std::slice::from_ref(&outside)),
            "export PATH=\"/opt/porta/bin:$PATH\""
        );

        let fish = fish_export_block(&[under_home, outside]);
        assert_eq!(
            fish,
            "fish_add_path --prepend \"$HOME/.porta/bin\" \"/opt/porta/bin\""
        );
    }

    #[test]
    fn write_block_preserves_existing_content() {
        let dir = std::env::temp_dir().join(format!("porta-test2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rc");
        std::fs::write(&file, "echo hello\n").unwrap();

        write_block(&file, "export PATH=\"/a:$PATH\"").unwrap();

        let contents = std::fs::read_to_string(&file).unwrap();
        assert!(contents.starts_with("echo hello\n"));
        assert!(contents.contains(BEGIN_MARKER));

        std::fs::remove_dir_all(&dir).ok();
    }
}
