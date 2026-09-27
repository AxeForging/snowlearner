//! Command-line interface. `snowlearner` with no subcommand starts the app.

mod commands;

use crate::config::level::Commitment;
use crate::config::settings::WindowMode;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "snowlearner",
    version,
    about = "A pixel-art frost mage freezes your screen until you practice speaking a language."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Cmd>,
    #[command(flatten)]
    pub run: RunArgs,
}

#[derive(clap::Args, Debug, Clone, Default)]
pub struct RunArgs {
    /// Window mode (overrides config).
    #[arg(long, value_enum, global = true)]
    pub mode: Option<WindowMode>,
    /// Commitment level / compromisso (overrides config).
    #[arg(long, value_enum, global = true)]
    pub level: Option<Commitment>,
    /// Deck/language to practice, e.g. en or es (overrides config).
    #[arg(long, global = true)]
    pub learning: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Start the app (default).
    Run,
    /// Ask the running app for a phrase challenge (bind this to a desktop shortcut on Wayland).
    Say,
    /// Show the end-of-day recap in the running app.
    Summary,
    /// Open the control panel in the running app.
    Menu,
    /// Register desktop shortcuts for the hotkeys (GNOME; needed on Wayland).
    Shortcuts {
        #[command(subcommand)]
        action: ShortcutAction,
    },
    /// Pause / resume the frost mage in the running app.
    Pause,
    /// Close the running app.
    Quit,
    /// Print what you practiced on a day (works without the app running).
    Report {
        /// Day as YYYY-MM-DD (default: today).
        #[arg(long)]
        date: Option<String>,
        /// Also read it aloud.
        #[arg(long)]
        speak: bool,
    },
    /// List the phrases of a deck.
    Decks,
    /// Manage the config file.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage the offline speech-recognition model.
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Check what works on this machine and how to fix what doesn't.
    Doctor,
    /// Render the scene to a PNG without opening a window.
    Snapshot {
        out: PathBuf,
        /// Simulated seconds before the frame is captured.
        #[arg(long, default_value_t = 60.0)]
        seconds: f32,
        #[arg(long, default_value_t = 320)]
        width: i32,
        #[arg(long, default_value_t = 180)]
        height: i32,
        /// PNG pixels per art pixel.
        #[arg(long, default_value_t = 3)]
        scale: u32,
        /// Transparent background, like the desktop overlay.
        #[arg(long)]
        overlay: bool,
        /// Show a lesson caption in the frame.
        #[arg(long)]
        caption: bool,
        #[arg(long, default_value_t = 7)]
        seed: u64,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Write a default config file.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Print where config, decks, history and models live.
    Path,
}

#[derive(Subcommand, Debug)]
pub enum ShortcutAction {
    /// Add the snowlearner shortcuts to GNOME (keeps your other shortcuts).
    Install,
    /// Remove the snowlearner shortcuts.
    Remove,
}

#[derive(Subcommand, Debug)]
pub enum ModelAction {
    /// Download a whisper model (tiny ~75MB, base ~142MB, small ~466MB).
    Download {
        /// Defaults to the `model` setting.
        #[arg(long)]
        name: Option<String>,
    },
    /// Print the model path and whether it exists.
    Path,
}

pub fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    commands::dispatch(cli)
}
