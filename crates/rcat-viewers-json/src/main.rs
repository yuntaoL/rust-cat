//! rcat-viewer-json — External plugin binary for viewing JSON files.

use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};

use rcat_core::FileViewer;
use rcat_core::dump::DumpOptions;
use rcat_core::file_info::FileInfo;
use rcat_core::plugin::{
    PluginCapability, PluginDefaultPriority, PluginHandles, PluginInfo, PluginRequest,
    PluginResponse,
};
use rcat_core::view::PositionKind;
use rcat_core::viewer::ViewerPriority;
use rcat_viewers_json::JsonViewerLogic;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

fn main() {
    init_logging();

    let args: Vec<String> = std::env::args().collect();

    if args.len() == 1 {
        if io::stdin().is_terminal() {
            print_usage_and_exit();
        }
        if let Err(e) = run_protocol_once() {
            tracing::error!("Protocol error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if args[1] == "--plugin-info" {
        print_plugin_info();
        return;
    }

    if args.len() >= 3 && args[1] == "dump" {
        if let Err(e) = handle_dump(&args[2], &DumpOptions::default()) {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if !io::stdin().is_terminal() {
        if let Err(e) = run_protocol_once() {
            tracing::error!("Protocol error: {e}");
            std::process::exit(1);
        }
        return;
    }

    eprintln!("Unknown command: {}", args[1]);
    eprintln!("Try --plugin-info or dump <file>");
    std::process::exit(2);
}

fn print_usage_and_exit() -> ! {
    eprintln!("rcat-viewer-json — JSON viewer plugin for rcat");
    eprintln!();
    eprintln!("Shows the file as raw bytes with JSON syntax highlighting (no reformat).");
    eprintln!();
    eprintln!("  rcat-viewer-json --plugin-info");
    eprintln!("  rcat-viewer-json dump <file>");
    std::process::exit(0);
}

fn init_logging() {
    let filter = if let Ok(v) = std::env::var("RCAT_LOG") {
        EnvFilter::new(v)
    } else if let Ok(v) = std::env::var("RUST_LOG") {
        EnvFilter::new(v)
    } else {
        EnvFilter::new("rcat_viewers_json=info")
    };

    let log_file_env = std::env::var("RCAT_LOG_FILE").ok();

    if let Some(ref path_str) = log_file_env {
        let path = PathBuf::from(path_str);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
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
                eprintln!(
                    "Warning (rcat-viewer-json): failed to open RCAT_LOG_FILE {:?}: {e}",
                    path
                );
                let stderr_layer = fmt::layer().with_writer(io::stderr).with_ansi(true);
                tracing_subscriber::registry()
                    .with(filter)
                    .with(stderr_layer)
                    .init();
            }
        }
    } else {
        let stderr_layer = fmt::layer().with_writer(io::stderr).with_ansi(true);
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .init();
    }
}

fn print_plugin_info() {
    let info = PluginInfo {
        name: "JSON".to_string(),
        version: "0.4.0".to_string(),
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
        position_kind: Some(PositionKind::Byte),
    };

    println!("{}", serde_json::to_string_pretty(&info).unwrap());
}

fn handle_dump(path: &str, opts: &DumpOptions) -> io::Result<()> {
    let info = FileInfo::from_path(path)?;
    let viewer = JsonViewerLogic;
    let mut stdout = io::stdout();
    viewer.dump(&info, &mut stdout, opts)
}

fn run_protocol_once() -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty plugin request",
        ));
    }

    let request: PluginRequest = serde_json::from_str(&line)?;
    let response = handle_request(&request)?;
    let response_json = serde_json::to_string(&response)?;
    writeln!(io::stdout(), "{response_json}")?;
    io::stdout().flush()?;
    Ok(())
}

fn handle_request(request: &PluginRequest) -> io::Result<PluginResponse> {
    let logic = JsonViewerLogic;

    Ok(match request {
        PluginRequest::CanHandle {
            preliminary,
            initial_data,
            ..
        } => {
            let looks_like_json = preliminary.mime_type.as_deref() == Some("application/json")
                || preliminary.extension.as_deref() == Some("json")
                || looks_like_json_data(initial_data);

            let priority = if looks_like_json {
                ViewerPriority::Preferred
            } else {
                ViewerPriority::None
            };
            PluginResponse::CanHandleResult { priority }
        }

        PluginRequest::RenderLines {
            file_path,
            start_offset,
            max_rows,
            width,
        } => {
            let info = FileInfo::from_path(file_path)?;
            let lines = logic.render_lines(&info, *start_offset, *max_rows, *width);
            PluginResponse::RenderLinesResult { lines }
        }

        PluginRequest::AdvanceLines {
            file_path,
            current,
            delta,
            width,
        } => {
            let info = FileInfo::from_path(file_path)?;
            let position = logic.advance_lines(&info, *current, *delta, *width);
            PluginResponse::AdvanceLinesResult { position }
        }

        PluginRequest::Status {
            file_path,
            position,
        } => {
            let info = FileInfo::from_path(file_path)?;
            let status = logic.status(&info, *position);
            PluginResponse::StatusResult { status }
        }

        PluginRequest::Dump {
            file_path,
            offset,
            length,
        } => {
            let opts = DumpOptions {
                offset: *offset,
                length: *length,
            };
            let mut buf = Vec::new();
            let info = FileInfo::from_path(file_path)?;
            logic.dump(&info, &mut buf, &opts)?;
            PluginResponse::DumpResult {
                output: String::from_utf8_lossy(&buf).into_owned(),
            }
        }

        PluginRequest::ByteAtDisplayLine { .. } | PluginRequest::DisplayLineAtByte { .. } => {
            PluginResponse::Error {
                message: "JSON viewer uses byte offsets only (raw file view)".to_string(),
            }
        }

        PluginRequest::ReadBytes { .. } => PluginResponse::Error {
            message: "ReadBytes not supported by rcat-viewer-json".to_string(),
        },
    })
}

fn looks_like_json_data(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let s = String::from_utf8_lossy(data);
    let trimmed = s.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}
