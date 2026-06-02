//! rcat — the rust-cat binary
//!
//! A modern, extensible terminal file viewer for text and binary files.

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::generate;
use rcat_core::FileSession;
use rcat_core::plugin::PluginCapability;
use rcat_core::probe::{FileProbeWithInfo, PrefixProbe};
use rcat_core::{ViewerRegistry, dump};
use rcat_viewers_hex::HexViewer;
use rcat_viewers_json::JsonViewerLogic;
use rcat_viewers_text::TextViewer;
use std::fs::OpenOptions;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
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
                  Examples:\n  rcat README.md\n  rcat --hex /bin/ls\n  rcat --offset 0x1000 --length 256 firmware.bin\n  \
                  echo '{\"a\":1}' | rcat"
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
#[allow(clippy::enum_variant_names)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let app_config = rcat_cli::config::RcatConfig::load(cli.config.as_deref());

    init_logging(cli.verbose, cli.log_file.clone());

    if cli.list_viewers {
        list_viewers();
        return Ok(());
    }

    if let Some(Commands::Completions { shell }) = cli.command {
        print_completions(shell);
        return Ok(());
    }

    let offset = rcat_cli::offset::parse_offset(&cli.offset)?;
    let mode = if cli.hex {
        ViewMode::Hex
    } else if cli.text {
        ViewMode::Text
    } else {
        ViewMode::Auto
    };
    let use_stdout = cli.stdout || !std::io::stdout().is_terminal();

    let (_stdin_guard, path) = match &cli.file {
        Some(p) => (None, p.clone()),
        None => {
            let (tmp, p) = rcat_cli::stdin::spool_stdin_to_temp()?;
            (Some(tmp), p)
        }
    };

    run_on_path(&path, mode, offset, cli.length, use_stdout, &app_config)
}

fn run_on_path(
    path: &Path,
    mode: ViewMode,
    offset: u64,
    length: Option<u64>,
    use_stdout: bool,
    app_config: &rcat_cli::config::RcatConfig,
) -> anyhow::Result<()> {
    let session = FileSession::open(path)?;

    let registry = build_registry(app_config)?;

    let prefix_probe = PrefixProbe::from_path(path)?;
    let mut probe = FileProbeWithInfo::new(session.info(), prefix_probe);

    let initial_viewer_index = match mode {
        ViewMode::Hex => registry.index_of("Hex").unwrap_or(0),
        ViewMode::Text => registry.index_of("Text").unwrap_or(0),
        ViewMode::Auto => {
            let best = registry
                .find_best(&mut probe)
                .expect("at least one viewer should be registered");
            tracing::info!(viewer = best.name(), "auto-selected viewer");
            registry.index_of(best.name()).unwrap_or(0)
        }
    };

    let selected_viewer = registry
        .all_viewers()
        .get(initial_viewer_index)
        .expect("initial_viewer_index is valid")
        .as_ref();

    tracing::debug!(
        viewer = selected_viewer.name(),
        use_stdout,
        "final viewer selected"
    );

    if use_stdout {
        let opts = dump::DumpOptions { offset, length };
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        selected_viewer.dump(session.info(), &mut lock, &opts)?;
    } else {
        let viewers = registry.into_viewers();
        if viewers.is_empty() {
            anyhow::bail!("no viewers registered");
        }

        rcat_tui::run_tui(rcat_tui::TuiConfig {
            session,
            viewers,
            initial_viewer_index,
            initial_offset: offset,
        })?;
    }

    Ok(())
}

fn build_registry(app_config: &rcat_cli::config::RcatConfig) -> anyhow::Result<ViewerRegistry> {
    let mut registry = ViewerRegistry::new();
    registry.register(Box::new(TextViewer));
    registry.register(Box::new(HexViewer));
    registry.register(Box::new(JsonViewerLogic));

    let search_paths = rcat_cli::plugin_discovery::plugin_search_paths();
    let discovered = rcat_cli::plugin_discovery::discover_plugins(&search_paths);
    tracing::info!(count = discovered.len(), "external plugins discovered");

    let timeout = app_config.plugin_timeout();

    for plugin_path in discovered {
        match rcat_core::external_plugin::ExternalPluginViewer::with_timeout(
            plugin_path.clone(),
            timeout,
        ) {
            Ok(plugin) => {
                let meta = plugin.info();
                let usable = meta.capabilities.contains(&PluginCapability::CanHandle)
                    && (meta.capabilities.contains(&PluginCapability::Dump)
                        || meta.capabilities.contains(&PluginCapability::RenderLines));
                if usable {
                    if meta.name == "JSON" && registry.index_of("JSON").is_some() {
                        tracing::debug!(
                            path = %plugin_path.display(),
                            "skipping external JSON plugin (in-process JSON viewer registered)"
                        );
                        continue;
                    }
                    tracing::debug!(
                        name = %meta.name,
                        version = %meta.version,
                        path = %plugin_path.display(),
                        "registering external plugin"
                    );
                    registry.register(Box::new(plugin));
                } else {
                    tracing::trace!(name = %meta.name, "plugin missing required capabilities");
                }
            }
            Err(e) => {
                tracing::warn!(path = %plugin_path.display(), error = %e, "failed to load plugin");
                eprintln!("Warning: Failed to load plugin {:?}: {e}", plugin_path);
            }
        }
    }

    Ok(registry)
}

fn list_viewers() {
    println!("Built-in viewers:");
    println!("  Text   — UTF-8 text pager");
    println!("  Hex    — 16-byte hex + ASCII");
    println!();
    println!("External plugins are discovered next to the rcat binary and in:");
    println!("  ~/.config/rcat/plugins/");
    println!();
    println!("Run `rcat <file>` to see which viewers are active for that file.");
}

fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let name = "rcat";
    match shell {
        Shell::Bash => generate(
            clap_complete::shells::Bash,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
        Shell::Zsh => generate(
            clap_complete::shells::Zsh,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
        Shell::Fish => generate(
            clap_complete::shells::Fish,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
        Shell::PowerShell => generate(
            clap_complete::shells::PowerShell,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
        Shell::Elvish => generate(
            clap_complete::shells::Elvish,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Auto,
    Text,
    Hex,
}

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

    if let Some(path) = &log_file {
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("Warning: failed to create log directory {:?}: {e}", parent);
        }

        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => {
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
                eprintln!("Warning: failed to open log file {:?}: {e}", path);
                let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_ansi(true);
                tracing_subscriber::registry()
                    .with(filter)
                    .with(stderr_layer)
                    .init();
            }
        }
    } else {
        let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_ansi(true);
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .init();
    }

    tracing::debug!(verbose, log_file = ?log_file, "logging initialized");
}


