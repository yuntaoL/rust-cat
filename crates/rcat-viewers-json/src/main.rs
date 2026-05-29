//! rcat-viewer-json — External plugin for viewing JSON files.
//!
//! This is the first external plugin used to validate the plugin system.

use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Read, Write};

use rcat_core::FileViewer;
use rcat_core::file_info::FileInfo;
use rcat_core::plugin::{
    PluginCapability, PluginDefaultPriority, PluginHandles, PluginInfo, PluginRequest,
    PluginResponse,
};
use rcat_core::viewer::ViewerPriority;
use serde_json::{Value, from_slice, to_string_pretty};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

fn main() {
    // Initialize logging to stderr only (never stdout, to protect the JSON protocol).
    // If RCAT_LOG_FILE is set (usually by the rcat host), we also append to that file
    // so that host + plugin logs are merged in one place.
    let filter = if let Ok(v) = std::env::var("RCAT_LOG") {
        EnvFilter::new(v)
    } else if let Ok(v) = std::env::var("RUST_LOG") {
        EnvFilter::new(v)
    } else {
        EnvFilter::new("rcat_viewers_json=info")
    };

    // Same policy as the host:
    // - If RCAT_LOG_FILE is set (host told us where to log), we write *only* to the file.
    //   Never to stderr — this protects both the JSON protocol and the host TUI.
    // - Otherwise we write only to stderr (normal standalone plugin behavior).
    let log_file_env = std::env::var("RCAT_LOG_FILE").ok();

    if let Some(ref path_str) = log_file_env {
        let path = std::path::PathBuf::from(path_str);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                // Quiet announcement when driven by host (will go to the file itself)
                tracing::debug!("plugin also logging to {}", path.display());

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
                // Hard failure case — we have no choice but to complain to stderr
                eprintln!(
                    "Warning (rcat-viewer-json): failed to open RCAT_LOG_FILE {:?}: {}",
                    path, e
                );
                // Fallback to stderr
                let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_ansi(true);
                tracing_subscriber::registry()
                    .with(filter)
                    .with(stderr_layer)
                    .init();
            }
        }
    } else {
        // No log file requested → classic stderr-only behavior
        let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_ansi(true);
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .init();
    }

    tracing::debug!("rcat-viewer-json starting");

    let args: Vec<String> = std::env::args().collect();

    if args.len() <= 1 {
        // No arguments — this is the common case when a user runs the plugin directly.
        // We should never hang on stdin in this situation.
        eprintln!("rcat-viewer-json — JSON viewer plugin for rcat");
        eprintln!();
        eprintln!("This binary is meant to be invoked by the rcat host, not run directly.");
        eprintln!();
        eprintln!("Available commands (for development/testing):");
        eprintln!("  rcat-viewer-json --plugin-info");
        eprintln!("  rcat-viewer-json dump <file>");
        eprintln!();
        eprintln!("When used as a plugin, the host communicates via JSON over stdin/stdout.");
        eprintln!();
        eprintln!(
            "Logging: set RCAT_LOG=debug (or RUST_LOG). Use RCAT_LOG_FILE to also write to a file."
        );
        std::process::exit(0);
    }

    if args[1] == "--plugin-info" {
        print_plugin_info();
        return;
    }

    // Simple mode for testing: support "dump <file>"
    if args.len() >= 3 && args[1] == "dump" {
        let path = &args[2];
        if let Err(e) = handle_dump(path) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Default: run as protocol handler (reads JSON requests from stdin)
    // This is the normal path when invoked by the rcat host.
    tracing::info!("Starting in protocol mode (waiting for JSON on stdin)");
    if let Err(e) = run_protocol() {
        tracing::error!("Protocol error: {}", e);
        std::process::exit(1);
    }
}

fn print_plugin_info() {
    tracing::debug!("Responding to --plugin-info");

    let info = PluginInfo {
        name: "JSON".to_string(),
        version: "0.2.0".to_string(),
        protocol_version: "1".to_string(),
        capabilities: vec![
            PluginCapability::CanHandle,
            PluginCapability::Dump,
            PluginCapability::RenderLines,
        ],
        handles: PluginHandles {
            extensions: vec!["json".to_string()],
            mime_types: vec!["application/json".to_string()],
            magic: vec![],
        },
        default_priority: PluginDefaultPriority::Preferred,
    };

    println!("{}", serde_json::to_string_pretty(&info).unwrap());
}

fn handle_dump(path: &str) -> io::Result<()> {
    let info = FileInfo::from_path(path)?;
    let viewer = JsonViewerLogic;
    let mut stdout = std::io::stdout();
    let opts = rcat_core::dump::DumpOptions::default();
    viewer.dump(&info, &mut stdout, &opts)
}

/// Very basic JSON logic extracted so it can be reused.
struct JsonViewerLogic;

impl JsonViewerLogic {
    fn pretty_lines(&self, info: &FileInfo) -> Vec<String> {
        if info.size == 0 {
            return vec!["(empty file)".to_string()];
        }

        let mut file = match std::fs::File::open(&info.path) {
            Ok(f) => f,
            Err(_) => return vec!["(error opening file)".to_string()],
        };

        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            return vec!["(read error)".to_string()];
        }

        let pretty = match from_slice::<Value>(&buf) {
            Ok(value) => to_string_pretty(&value)
                .unwrap_or_else(|_| String::from_utf8_lossy(&buf).into_owned()),
            Err(_) => {
                let mut s = String::from_utf8_lossy(&buf).into_owned();
                s.insert_str(0, "(not valid JSON — showing raw)\n");
                s
            }
        };

        pretty.lines().map(|l| l.to_string()).collect()
    }
}

impl rcat_core::FileViewer for JsonViewerLogic {
    fn name(&self) -> &'static str {
        "JSON"
    }

    fn can_handle(&self, _probe: &mut dyn rcat_core::probe::FileProbe) -> ViewerPriority {
        // This implementation is only used internally by the plugin binary.
        // The real can_handle decision happens in the protocol handler below.
        ViewerPriority::Preferred
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        _opts: &rcat_core::dump::DumpOptions,
    ) -> io::Result<()> {
        let lines = self.pretty_lines(info);
        for line in lines {
            writeln!(writer, "{}", line)?;
        }
        Ok(())
    }
}

/// Very basic protocol handler (for initial testing of the plugin system).
fn run_protocol() -> io::Result<()> {
    let stdin = io::stdin();
    let reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout();

    tracing::debug!("Entered protocol read loop");

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        tracing::trace!(request = %line, "Received request from host");

        let response = match serde_json::from_str::<PluginRequest>(&line) {
            Ok(PluginRequest::CanHandle {
                file_size: _,
                preliminary,
                initial_data,
            }) => {
                tracing::debug!(
                    mime = ?preliminary.mime_type,
                    ext = ?preliminary.extension,
                    "Received CanHandle request"
                );

                let looks_like_json = preliminary.mime_type.as_deref() == Some("application/json")
                    || preliminary.extension.as_deref() == Some("json")
                    || looks_like_json_data(&initial_data);

                let priority = if looks_like_json {
                    ViewerPriority::Preferred
                } else {
                    ViewerPriority::None
                };

                tracing::debug!(?priority, "Responding to CanHandle");

                PluginResponse::CanHandleResult { priority }
            }

            Ok(PluginRequest::ReadBytes { .. }) => PluginResponse::Error {
                message: "ReadBytes not supported in this basic plugin version".to_string(),
            },

            Err(e) => PluginResponse::Error {
                message: format!("Failed to parse request: {}", e),
            },
        };

        let response_json = serde_json::to_string(&response)?;
        writeln!(stdout, "{}", response_json)?;
        stdout.flush()?;
    }

    Ok(())
}

fn looks_like_json_data(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let s = String::from_utf8_lossy(data);
    let trimmed = s.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}
