//! Adapter that turns an external command-line plugin into a `FileViewer`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::dump::DumpOptions;
use crate::file_info::FileInfo;
use crate::plugin::{
    DEFAULT_PLUGIN_TIMEOUT_SECS, PluginCapability, PluginInfo, PluginRequest, PluginResponse,
};
use crate::probe::FileProbe;
use crate::view::{PositionKind, ViewAnchor, ViewContext, ViewportResult};
use crate::viewer::{FileViewer, ViewerPriority};
use tracing::{debug, trace, warn};

/// Forward logging-related environment variables to child plugin processes.
fn configure_logging_for_child(cmd: &mut Command) {
    for var in ["RCAT_LOG_FILE", "RCAT_LOG", "RUST_LOG"] {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }
}

/// Spawn the plugin, send one JSON request on stdin, read one JSON response from stdout.
pub fn run_plugin_request(
    executable: &Path,
    request: &PluginRequest,
    timeout: Duration,
) -> std::io::Result<PluginResponse> {
    debug!(path = %executable.display(), ?request, "spawning plugin for request");

    let mut cmd = Command::new(executable);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    configure_logging_for_child(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| {
        warn!(path = %executable.display(), error = %e, "failed to spawn plugin");
        e
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        let request_json = serde_json::to_string(request).map_err(std::io::Error::other)?;
        trace!(request = %request_json, "writing JSON to plugin stdin");
        writeln!(stdin, "{request_json}")?;
        let _ = stdin.flush();
    }

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    std::io::Read::read_to_end(&mut out, &mut stdout)?;
                }
                if let Some(mut err) = child.stderr.take() {
                    std::io::Read::read_to_end(&mut err, &mut stderr)?;
                }
                if !status.success() {
                    warn!(
                        stderr = %String::from_utf8_lossy(&stderr),
                        "plugin exited non-zero"
                    );
                    return Err(std::io::Error::other(format!(
                        "Plugin {:?} failed: {}",
                        executable,
                        String::from_utf8_lossy(&stderr)
                    )));
                }
                let response: PluginResponse = serde_json::from_slice(&stdout).map_err(|e| {
                    std::io::Error::other(format!(
                        "Invalid plugin JSON response: {e}; stdout={}",
                        String::from_utf8_lossy(&stdout)
                    ))
                })?;
                return Ok(response);
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                warn!(path = %executable.display(), ?timeout, "plugin request timed out");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("Plugin {:?} timed out after {:?}", executable, timeout),
                ));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn path_str(info: &FileInfo) -> String {
    info.path.display().to_string()
}

/// An external viewer plugin discovered at runtime.
pub struct ExternalPluginViewer {
    executable: PathBuf,
    info: PluginInfo,
    timeout: Duration,
}

impl ExternalPluginViewer {
    /// Create a new external plugin from a discovered executable.
    pub fn new(executable: PathBuf) -> std::io::Result<Self> {
        Self::with_timeout(executable, Duration::from_secs(DEFAULT_PLUGIN_TIMEOUT_SECS))
    }

    pub fn with_timeout(executable: PathBuf, timeout: Duration) -> std::io::Result<Self> {
        let info = Self::query_plugin_info(&executable)?;
        Ok(Self {
            executable,
            info,
            timeout,
        })
    }

    fn query_plugin_info(executable: &Path) -> std::io::Result<PluginInfo> {
        debug!(path = %executable.display(), "querying --plugin-info from external plugin");

        let mut cmd = Command::new(executable);
        cmd.arg("--plugin-info");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        configure_logging_for_child(&mut cmd);

        let output = cmd.output()?;

        if !output.status.success() {
            warn!(path = %executable.display(), stderr = %String::from_utf8_lossy(&output.stderr), "plugin --plugin-info failed");
            return Err(std::io::Error::other(format!(
                "Plugin {:?} failed --plugin-info: {}",
                executable,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let info: PluginInfo =
            serde_json::from_slice(&output.stdout).map_err(std::io::Error::other)?;

        debug!(name = %info.name, version = %info.version, "plugin info received");
        Ok(info)
    }

    pub fn info(&self) -> &PluginInfo {
        &self.info
    }

    pub fn has_capability(&self, cap: PluginCapability) -> bool {
        self.info.capabilities.contains(&cap)
    }

    fn invoke(&self, request: PluginRequest) -> std::io::Result<PluginResponse> {
        run_plugin_request(&self.executable, &request, self.timeout)
    }
}

impl FileViewer for ExternalPluginViewer {
    fn name(&self) -> &'static str {
        Box::leak(self.info.name.clone().into_boxed_str())
    }

    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority {
        let file_size = probe.file_size();
        let preliminary = probe.preliminary().clone();
        let initial_data: Vec<u8> = probe.read_bytes(0, 16 * 1024).unwrap_or_default().to_vec();

        let request = PluginRequest::CanHandle {
            file_size,
            preliminary,
            initial_data,
        };

        match self.invoke(request) {
            Ok(PluginResponse::CanHandleResult { priority }) => {
                debug!(plugin = self.info.name, ?priority, "plugin CanHandle");
                priority
            }
            Ok(PluginResponse::Error { message }) => {
                warn!(plugin = self.info.name, %message, "plugin CanHandle error");
                ViewerPriority::None
            }
            Ok(_) => {
                warn!(plugin = self.info.name, "unexpected CanHandle response");
                ViewerPriority::None
            }
            Err(e) => {
                warn!(plugin = self.info.name, error = %e, "CanHandle request failed");
                ViewerPriority::None
            }
        }
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        if self.has_capability(PluginCapability::Dump) {
            let request = PluginRequest::Dump {
                file_path: path_str(info),
                offset: opts.offset,
                length: opts.length,
            };
            match self.invoke(request) {
                Ok(PluginResponse::DumpResult { output }) => {
                    writer.write_all(output.as_bytes())?;
                    return Ok(());
                }
                Ok(PluginResponse::Error { message }) => {
                    return Err(std::io::Error::other(message));
                }
                Ok(_) | Err(_) => {
                    debug!(
                        plugin = self.info.name,
                        "protocol Dump failed, trying CLI dump"
                    );
                }
            }
        }

        debug!(plugin = self.info.name, "invoking plugin dump (CLI mode)");
        let mut cmd = Command::new(&self.executable);
        cmd.arg("dump");
        cmd.arg(&info.path);
        configure_logging_for_child(&mut cmd);

        let output = cmd.output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "Plugin {} failed: {}",
                self.name(),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        writer.write_all(&output.stdout)?;
        Ok(())
    }

    fn position_kind(&self) -> PositionKind {
        self.info.position_kind.unwrap_or(PositionKind::Byte)
    }

    fn scroll_extent(&self, info: &FileInfo) -> u64 {
        match self.position_kind() {
            PositionKind::DisplayLine => {
                if self.has_capability(PluginCapability::RenderLines) {
                    let status = self.status(info, 0);
                    crate::scroll::parse_display_line_extent_from_status(&status).unwrap_or(1)
                } else {
                    1
                }
            }
            PositionKind::Byte | PositionKind::Frame => info.size.saturating_sub(1),
        }
    }

    fn render_viewport(&self, ctx: &ViewContext) -> ViewportResult {
        let anchor = ctx.anchor;
        let info = ctx.info();
        let lines = self.render_lines(info, ctx.anchor_raw(), ctx.max_rows, ctx.content_width);
        let status = self.status(info, ctx.anchor_raw());
        let source_byte = self.source_byte_for_anchor(info, anchor);
        ViewportResult {
            lines,
            status,
            anchor,
            source_byte,
        }
    }

    fn source_byte_for_anchor(&self, info: &FileInfo, anchor: ViewAnchor) -> Option<u64> {
        match anchor {
            ViewAnchor::Byte(b) => Some(b.min(info.size.saturating_sub(1))),
            ViewAnchor::DisplayLine(line) => {
                let request = PluginRequest::ByteAtDisplayLine {
                    file_path: path_str(info),
                    line,
                };
                match self.invoke(request) {
                    Ok(PluginResponse::ByteAtDisplayLineResult { byte_offset }) => {
                        Some(byte_offset.min(info.size.saturating_sub(1)))
                    }
                    _ => None,
                }
            }
            ViewAnchor::Frame(_) => None,
        }
    }

    fn display_line_for_byte(&self, info: &FileInfo, byte: u64) -> Option<u64> {
        if self.position_kind() != PositionKind::DisplayLine {
            return None;
        }
        let request = PluginRequest::DisplayLineAtByte {
            file_path: path_str(info),
            byte: byte.min(info.size.saturating_sub(1)),
        };
        match self.invoke(request) {
            Ok(PluginResponse::DisplayLineAtByteResult { line }) => Some(line),
            _ => None,
        }
    }

    fn advance_anchor(&self, ctx: &ViewContext, delta: i64) -> ViewAnchor {
        let raw = self.advance_lines(ctx.info(), ctx.anchor_raw(), delta, ctx.content_width);
        ViewAnchor::from_raw(self.position_kind(), raw)
    }

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        width: u16,
    ) -> Vec<String> {
        if !self.has_capability(PluginCapability::RenderLines) {
            return vec![format!(
                "[{} viewer] render_lines not implemented (missing RenderLines capability)",
                self.name()
            )];
        }

        let request = PluginRequest::RenderLines {
            file_path: path_str(info),
            start_offset,
            max_rows,
            width,
        };

        match self.invoke(request) {
            Ok(PluginResponse::RenderLinesResult { lines }) => lines,
            Ok(PluginResponse::Error { message }) => {
                vec![format!("(plugin error: {message})")]
            }
            Ok(_) => vec![format!(
                "({} viewer: unexpected render_lines response)",
                self.name()
            )],
            Err(e) => vec![format!("(plugin error: {e})")],
        }
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, width: u16) -> u64 {
        if !self.has_capability(PluginCapability::RenderLines) {
            return FileViewer::advance_lines(self, info, current, delta, width);
        }

        let request = PluginRequest::AdvanceLines {
            file_path: path_str(info),
            current,
            delta,
            width,
        };

        match self.invoke(request) {
            Ok(PluginResponse::AdvanceLinesResult { position }) => position,
            _ => (current as i64 + delta).max(0) as u64,
        }
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        if !self.has_capability(PluginCapability::RenderLines) {
            return format!("{} @ {}", self.name(), pos);
        }

        let request = PluginRequest::Status {
            file_path: path_str(info),
            position: pos,
        };

        match self.invoke(request) {
            Ok(PluginResponse::StatusResult { status }) => status,
            _ => format!("{} @ {}", self.name(), pos),
        }
    }
}
