//! rcat-viewer-json — External plugin for viewing JSON files.

use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use rcat_core::FileViewer;
use rcat_core::dump::DumpOptions;
use rcat_core::file_info::FileInfo;
use rcat_core::plugin::{
    PluginCapability, PluginDefaultPriority, PluginHandles, PluginInfo, PluginRequest,
    PluginResponse,
};
use rcat_core::viewer::ViewerPriority;
use serde_json::{Value, from_slice, to_string_pretty};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

fn main() {
    init_logging();

    let args: Vec<String> = std::env::args().collect();

    // Host-driven protocol: spawned as `rcat-viewer-json` with JSON on stdin (no subcommand).
    // Interactive use with no args: show help instead of waiting on stdin.
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

    // Unknown subcommand — if stdin is piped, try protocol (defensive).
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
    eprintln!("Invoked by the rcat host via JSON on stdin/stdout, or for testing:");
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
        version: "0.3.0".to_string(),
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

fn handle_dump(path: &str, opts: &DumpOptions) -> io::Result<()> {
    let info = FileInfo::from_path(path)?;
    let viewer = JsonViewerLogic;
    let mut stdout = io::stdout();
    viewer.dump(&info, &mut stdout, opts)
}

/// Read one JSON line from stdin, write one JSON line to stdout, exit.
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
            width: _,
        } => {
            let lines = logic.render_lines_at(Path::new(file_path), *start_offset, *max_rows)?;
            PluginResponse::RenderLinesResult { lines }
        }

        PluginRequest::AdvanceLines {
            file_path,
            current,
            delta,
            width: _,
        } => {
            let position = logic.advance_lines_at(Path::new(file_path), *current, *delta)?;
            PluginResponse::AdvanceLinesResult { position }
        }

        PluginRequest::Status {
            file_path,
            position,
        } => {
            let status = logic.status_at(Path::new(file_path), *position)?;
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

struct JsonViewerLogic;

impl JsonViewerLogic {
    fn pretty_lines_from_path(&self, path: &Path) -> io::Result<Vec<String>> {
        let info = FileInfo::from_path(path)?;
        if info.size == 0 {
            return Ok(vec!["(empty file)".to_string()]);
        }

        let mut file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let pretty = match from_slice::<Value>(&buf) {
            Ok(value) => to_string_pretty(&value)
                .unwrap_or_else(|_| String::from_utf8_lossy(&buf).into_owned()),
            Err(_) => {
                let mut s = String::from_utf8_lossy(&buf).into_owned();
                s.insert_str(0, "(not valid JSON — showing raw)\n");
                s
            }
        };

        Ok(pretty.lines().map(|l| l.to_string()).collect())
    }

    fn line_count(&self, path: &Path) -> io::Result<usize> {
        Ok(self.pretty_lines_from_path(path)?.len().max(1))
    }

    fn render_lines_at(
        &self,
        path: &Path,
        start_offset: u64,
        max_rows: u16,
    ) -> io::Result<Vec<String>> {
        let all = self.pretty_lines_from_path(path)?;
        let start = start_offset as usize;
        let lines: Vec<String> = all
            .iter()
            .skip(start)
            .take(max_rows as usize)
            .cloned()
            .collect();
        Ok(if lines.is_empty() {
            vec!["(end of file)".to_string()]
        } else {
            lines
        })
    }

    fn advance_lines_at(&self, path: &Path, current: u64, delta: i64) -> io::Result<u64> {
        let total = self.line_count(path)?;
        let max_pos = total.saturating_sub(1) as u64;
        let new_pos = (current as i64 + delta).max(0) as u64;
        Ok(new_pos.min(max_pos))
    }

    fn status_at(&self, path: &Path, position: u64) -> io::Result<String> {
        let total = self.line_count(path)?;
        let pct = if total == 0 {
            0
        } else {
            ((position as f64 / total as f64) * 100.0) as u32
        };
        Ok(format!(
            "JSON  line {}/{} ({pct}%)",
            position + 1,
            total.max(1)
        ))
    }
}

impl FileViewer for JsonViewerLogic {
    fn name(&self) -> &'static str {
        "JSON"
    }

    fn can_handle(&self, _probe: &mut dyn rcat_core::probe::FileProbe) -> ViewerPriority {
        ViewerPriority::Preferred
    }

    fn dump(&self, info: &FileInfo, writer: &mut dyn Write, opts: &DumpOptions) -> io::Result<()> {
        let all = self.pretty_lines_from_path(&info.path)?;
        let start = opts.offset as usize;
        let end = match opts.length {
            Some(len) => (start + len as usize).min(all.len()),
            None => all.len(),
        };
        for line in &all[start..end] {
            writeln!(writer, "{line}")?;
        }
        Ok(())
    }
}
