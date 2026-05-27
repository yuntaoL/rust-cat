//! rcat — the rust-cat binary
//!
//! A modern, extensible terminal file viewer for text and binary files.

use clap::{Parser, Subcommand, ValueEnum};
use rcat_core::file_info::FileInfo;
use rcat_core::probe::{FileProbeWithInfo, PrefixProbe};
use rcat_core::{FileViewer, ViewerRegistry, dump};
use rcat_viewers_hex::HexViewer;
use rcat_viewers_json::JsonViewer;
use rcat_viewers_text::TextViewer;
use std::io::IsTerminal;
use std::path::PathBuf;

/// rcat - A modern, extensible terminal file viewer (text, hex, and beyond)
#[derive(Parser, Debug)]
#[command(
    name = "rcat",
    version,
    about = "A modern, extensible terminal file viewer — text, hex, and beyond",
    long_about = "rcat combines the best of cat, less, and hexyl into one fast, keyboard-driven tool.\n\n\
                  By default it opens an interactive TUI when stdout is a tty.\n\
                  Use --stdout (or pipe) for non-interactive output.",
    after_help = "Press '?' inside the TUI for keybindings.\n\n\
                  Examples:\n  rcat README.md\n  rcat --hex /bin/ls\n  rcat --offset 0x1000 --length 256 firmware.bin"
)]
struct Cli {
    /// File to view (omit to read from stdin)
    #[arg(value_name = "FILE", required = false)]
    file: Option<PathBuf>,

    /// Force hex viewer
    #[arg(short = 'H', long, conflicts_with = "text")]
    hex: bool,

    /// Force text viewer
    #[arg(short = 'T', long, conflicts_with = "hex")]
    text: bool,

    /// Start at byte offset (decimal or 0x-prefixed hex)
    #[arg(short, long, value_name = "OFFSET", default_value = "0")]
    offset: String,

    /// Limit number of bytes/lines to render
    #[arg(short, long, value_name = "LEN")]
    length: Option<u64>,

    /// Force non-interactive dump mode (useful in scripts / pipes)
    #[arg(long)]
    stdout: bool,

    /// Path to alternate config file
    #[arg(short, long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// List all registered viewers and exit
    #[arg(long)]
    list_viewers: bool,

    /// Increase verbosity (can be repeated)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate shell completions
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy)]
#[allow(clippy::enum_variant_names)] // Shell::Bash, Shell::Zsh etc. are the canonical names we want
enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.list_viewers {
        println!("Built-in viewers (v0.1):");
        println!("  text   - UTF-8 text pager with graceful fallback");
        println!("  hex    - Classic 16-byte hex + ASCII view (color-coded)");
        println!("  json   - Pretty-printed JSON (specialized plugin example)");
        println!("\nMore viewers will be available via plugins in future releases.");
        return Ok(());
    }

    if let Some(Commands::Completions { shell }) = cli.command {
        // Placeholder — real implementation uses clap_complete
        eprintln!(
            "Completions for {:?} not yet implemented in this build",
            shell
        );
        return Ok(());
    }

    // Parse offset (support 0x and decimal)
    let offset = parse_offset(&cli.offset)?;

    let mode = if cli.hex {
        ViewMode::Hex
    } else if cli.text {
        ViewMode::Text
    } else {
        ViewMode::Auto
    };

    let is_stdout_tty = std::io::stdout().is_terminal();
    let use_stdout = cli.stdout || !is_stdout_tty;

    if let Some(path) = &cli.file {
        let info = FileInfo::from_path(path)?;

        // Build registry once
        let mut registry = ViewerRegistry::new();
        registry.register(Box::new(TextViewer));
        registry.register(Box::new(HexViewer));
        registry.register(Box::new(JsonViewer));

        // Build probe for viewer selection (used in Auto mode)
        let prefix_probe = PrefixProbe::from_path(path)?;
        let mut probe = FileProbeWithInfo::new(&info, prefix_probe);

        let selected_viewer: &dyn FileViewer = match mode {
            ViewMode::Hex => registry
                .all_viewers()
                .iter()
                .find(|v| v.name() == "Hex")
                .map(|v| v.as_ref())
                .unwrap_or_else(|| registry.all_viewers().first().unwrap().as_ref()),
            ViewMode::Text => registry
                .all_viewers()
                .iter()
                .find(|v| v.name() == "Text")
                .map(|v| v.as_ref())
                .unwrap_or_else(|| registry.all_viewers().first().unwrap().as_ref()),
            ViewMode::Auto => registry
                .find_best(&mut probe)
                .expect("at least one viewer should be registered"),
        };

        if use_stdout {
            let opts = dump::DumpOptions {
                offset,
                length: cli.length,
            };

            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            selected_viewer.dump(&info, &mut lock, &opts)?;
        } else {
            // Launch the interactive TUI with the chosen viewer
            // We move the viewer into the TUI
            let viewer: Box<dyn rcat_core::FileViewer> = match selected_viewer.name() {
                "Hex" => Box::new(HexViewer),
                "JSON" => Box::new(JsonViewer),
                _ => Box::new(TextViewer),
            };

            let config = rcat_tui::TuiConfig {
                info,
                viewer,
                initial_offset: offset,
            };

            rcat_tui::run_tui(config)?;
        }
    } else {
        // stdin path (future)
        eprintln!("Reading from stdin is not fully supported yet. Please provide a file path.");
        std::process::exit(1);
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Auto,
    Text,
    Hex,
}

fn parse_offset(s: &str) -> anyhow::Result<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| anyhow::anyhow!("invalid hex offset: {}", e))
    } else {
        s.parse::<u64>()
            .map_err(|e| anyhow::anyhow!("invalid offset: {}", e))
    }
}
