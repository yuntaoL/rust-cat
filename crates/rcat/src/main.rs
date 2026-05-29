//! rcat — the rust-cat binary
//!
//! A modern, extensible terminal file viewer for text and binary files.

use clap::{Parser, Subcommand, ValueEnum};
use rcat_core::file_info::FileInfo;
use rcat_core::probe::{FileProbeWithInfo, PrefixProbe};
use rcat_core::{FileViewer, ViewerRegistry, dump};
use rcat_viewers_hex::HexViewer;
// JsonViewer is now provided as an external plugin (rcat-viewer-json)
use rcat_viewers_text::TextViewer;
use std::fs::OpenOptions;
use std::io::IsTerminal;
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

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

    /// Write logs to this file (in addition to stderr).
    /// Especially useful in interactive TUI mode — `tail -f` the file from another terminal to watch live logs.
    #[arg(long, value_name = "PATH", env = "RCAT_LOG_FILE")]
    log_file: Option<PathBuf>,

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

    init_logging(cli.verbose, cli.log_file.clone());

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
        // JSON viewer is provided via the external plugin system (rcat-viewer-json)

        // Discover external plugins (plug-and-play, no config needed)
        let search_paths = rcat_cli::plugin_discovery::plugin_search_paths();
        let discovered = rcat_cli::plugin_discovery::discover_plugins(&search_paths);

        tracing::info!(count = discovered.len(), "external plugins discovered");

        for plugin_path in discovered {
            match rcat_core::external_plugin::ExternalPluginViewer::new(plugin_path.clone()) {
                Ok(plugin) => {
                    let info = plugin.info();
                    // Only register plugins that declare they can do at least can_handle + dump
                    if info
                        .capabilities
                        .contains(&rcat_core::plugin::PluginCapability::CanHandle)
                    {
                        tracing::debug!(name = %info.name, version = %info.version, path = %plugin_path.display(), "registering external plugin");
                        registry.register(Box::new(plugin));
                    } else {
                        tracing::trace!(name = %info.name, "plugin lacks CanHandle capability, skipping");
                    }
                }
                Err(e) => {
                    tracing::warn!(path = %plugin_path.display(), error = %e, "failed to load plugin");
                    eprintln!("Warning: Failed to load plugin {:?}: {}", plugin_path, e);
                }
            }
        }

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
            ViewMode::Auto => {
                let best = registry
                    .find_best(&mut probe)
                    .expect("at least one viewer should be registered");
                tracing::info!(viewer = best.name(), "auto-selected viewer");
                best
            }
        };

        tracing::debug!(
            viewer = selected_viewer.name(),
            use_stdout,
            "final viewer selected"
        );

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
                _ => Box::new(TextViewer),
            };

            let config = rcat_tui::TuiConfig {
                info,
                viewer,
                initial_offset: offset,
            };

            // Note: we no longer emit any extra eprintln here.
            // When a log file is active we log *only* to the file (no stderr at all)
            // to guarantee the TUI is never corrupted by log output.
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

/// Initialize tracing subscriber.
///
/// - Always logs to stderr (never stdout, to protect JSON plugin protocol and TUI).
/// - If `log_file` is Some, also appends structured logs to that file (no ANSI colors).
/// - Prefers RCAT_LOG / RCAT_LOG_FILE over RUST_LOG.
/// - Falls back to level derived from -v/--verbose when no env vars are set.
///
/// This makes live debugging possible even when the TUI has taken over the screen.
fn init_logging(verbose: u8, log_file: Option<PathBuf>) {
    let filter = if let Ok(v) = std::env::var("RCAT_LOG") {
        EnvFilter::new(v)
    } else if let Ok(v) = std::env::var("RUST_LOG") {
        EnvFilter::new(v)
    } else {
        let level = match verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        };
        EnvFilter::new(format!(
            "rcat={l},rcat_core={l},rcat_cli={l},rcat_tui={l},rcat_viewers_text={l},rcat_viewers_hex={l},rcat_viewers_json={l}",
            l = level
        ))
    };

    // Policy: If the user asked for a log file (via --log-file or RCAT_LOG_FILE),
    // we log *only* to that file. Never to stderr. This is critical for TUI safety
    // because the TUI takes over the terminal (raw mode + alternate screen).
    // If no log file was requested, we log only to stderr (classic behavior).
    if let Some(path) = &log_file {
        // Best effort: create parent directories
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!(
                "Warning: failed to create log directory {:?}: {}",
                parent, e
            );
        }

        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => {
                // Make the log file visible to child processes (plugins) via env var.
                // SAFETY: early in main, single-threaded, intentional for child inheritance.
                unsafe {
                    std::env::set_var("RCAT_LOG_FILE", path);
                }
                eprintln!("Logging to {}", path.display());

                let file_layer = fmt::layer()
                    .with_writer(file)
                    .with_ansi(false)
                    .with_target(true);

                tracing_subscriber::registry()
                    .with(filter)
                    .with(file_layer)
                    .init();
            }
            Err(e) => {
                eprintln!("Warning: failed to open log file {:?}: {}", path, e);
                // Fallback to stderr-only so we don't lose all logs
                let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_ansi(true);
                tracing_subscriber::registry()
                    .with(filter)
                    .with(stderr_layer)
                    .init();
            }
        }
    } else {
        // Classic mode: only stderr, no file
        let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_ansi(true);
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .init();
    }

    tracing::debug!(verbose, log_file = ?log_file, "logging initialized");
}

fn parse_offset(s: &str) -> anyhow::Result<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| anyhow::anyhow!("invalid hex offset: {}", e))
    } else {
        s.parse::<u64>()
            .map_err(|e| anyhow::anyhow!("invalid offset: {}", e))
    }
}
