use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "porta",
    version,
    about = "A portable, no-admin developer environment — with a bundled AI coding CLI.",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Set up porta: create its directories and add its bin dir to PATH.
    Init {
        /// Also install the bundled AI CLI (Claude Code) right away.
        #[arg(long)]
        with_ai: bool,
    },
    /// Show environment status: PATH wiring, installed tools.
    Doctor,
    /// List tools available in the manifest and which are installed.
    List,
    /// Install a tool by name (see `porta list`).
    Install {
        /// Tool name from the manifest, e.g. `ai`, `ripgrep`.
        name: String,
        /// Force a specific install strategy instead of the manifest's
        /// default (script/binary auto-fallback-to-source).
        #[arg(long, value_parser = ["script", "binary", "source"])]
        strategy: Option<String>,
    },
    /// Remove a tool porta installed (does not touch vendor-managed
    /// installs from the `script` strategy, e.g. Claude Code's own updater).
    Uninstall { name: String },
    /// Print the PATH export line for shells porta doesn't auto-configure.
    Path,
    /// Print where an installed tool's binary lives.
    Which { name: String },
    /// Manage dotfiles stored inside the environment (they travel with it).
    Dotfiles {
        #[command(subcommand)]
        action: DotfilesAction,
    },
}

#[derive(Subcommand)]
pub enum DotfilesAction {
    /// Move file(s) into $PORTA_HOME/dotfiles and symlink them back into
    /// place, so they move with the environment.
    Add { paths: Vec<std::path::PathBuf> },
    /// (Re)create the $HOME symlinks for every tracked dotfile — run this
    /// after copying the environment to a new machine (porta init does it
    /// automatically).
    Link,
    /// Show tracked dotfiles and whether each is currently linked.
    List,
    /// Stop tracking a dotfile: restore a real copy to $HOME and remove it
    /// from the store.
    Remove { path: std::path::PathBuf },
}
