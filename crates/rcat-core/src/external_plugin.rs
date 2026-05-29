//! Adapter that turns an external command-line plugin into a `FileViewer`.

#![allow(dead_code)] // Plugin system is under active development

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::dump::DumpOptions;
use crate::file_info::FileInfo;
use crate::plugin::{PluginInfo, PluginRequest, PluginResponse};
use crate::probe::FileProbe;
use crate::viewer::{FileViewer, ViewerPriority};
use tracing::{debug, trace, warn};

/// Forward logging-related environment variables to child plugin processes.
/// This allows the plugin to write to the same --log-file (if any) that the
/// host is using, so all logs end up merged in one place.
fn configure_logging_for_child(cmd: &mut Command) {
    for var in ["RCAT_LOG_FILE", "RCAT_LOG", "RUST_LOG"] {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }
}

/// An external viewer plugin discovered at runtime.
pub struct ExternalPluginViewer {
    /// Path to the plugin executable.
    executable: PathBuf,
    /// Cached plugin metadata (fetched once).
    info: PluginInfo,
}

impl ExternalPluginViewer {
    /// Create a new external plugin from a discovered executable.
    /// This will immediately query `--plugin-info`.
    pub fn new(executable: PathBuf) -> std::io::Result<Self> {
        let info = Self::query_plugin_info(&executable)?;

        Ok(Self { executable, info })
    }

    fn query_plugin_info(executable: &PathBuf) -> std::io::Result<PluginInfo> {
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
}

impl FileViewer for ExternalPluginViewer {
    fn name(&self) -> &'static str {
        // We leak the string to satisfy the 'static lifetime required by the trait.
        // This is acceptable because plugin names are small and live for the whole process.
        Box::leak(self.info.name.clone().into_boxed_str())
    }

    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority {
        // Basic implementation of the plugin protocol for can_handle.
        // We send initial data (up to 16KB) and ask the plugin for its priority.

        let file_size = probe.file_size();
        let preliminary = probe.preliminary().clone();

        // Read up to 16KB for detection
        let initial_data: Vec<u8> = probe.read_bytes(0, 16 * 1024).unwrap_or_default().to_vec();

        debug!(
            plugin = self.info.name,
            file_size,
            initial_len = initial_data.len(),
            "sending CanHandle request to plugin"
        );

        let request = PluginRequest::CanHandle {
            file_size,
            preliminary,
            initial_data,
        };

        // Spawn the plugin and send the request
        let mut cmd = Command::new(&self.executable);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        configure_logging_for_child(&mut cmd);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                warn!(plugin = self.info.name, error = %e, "failed to spawn plugin for can_handle");
                return ViewerPriority::None;
            }
        };

        {
            let stdin = child.stdin.as_mut().expect("failed to open stdin");
            let request_json = serde_json::to_string(&request).unwrap();
            trace!(request = %request_json, "writing CanHandle JSON to plugin stdin");
            if writeln!(stdin, "{}", request_json).is_err() {
                return ViewerPriority::None;
            }
            let _ = stdin.flush();
        }

        let output = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                warn!(plugin = self.info.name, error = %e, "plugin wait failed");
                return ViewerPriority::None;
            }
        };

        if !output.status.success() {
            warn!(
                plugin = self.info.name,
                "plugin exited non-zero for CanHandle"
            );
            return ViewerPriority::None;
        }

        // The plugin should have printed a single JSON line with CanHandleResult
        if let Ok(PluginResponse::CanHandleResult { priority }) =
            serde_json::from_slice::<PluginResponse>(&output.stdout)
        {
            debug!(
                plugin = self.info.name,
                ?priority,
                "plugin responded to CanHandle"
            );
            return priority;
        }

        // Fallback
        warn!(
            plugin = self.info.name,
            "plugin returned unexpected response for CanHandle, falling back to Low"
        );
        ViewerPriority::Low
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        _opts: &DumpOptions,
    ) -> std::io::Result<()> {
        // For external plugins in the first version, we use a simple CLI interface:
        //   rcat-viewer-xxx dump <file>
        //
        // This is easy for plugin authors and works well for non-interactive use.
        debug!(plugin = self.info.name, path = %info.path.display(), "invoking plugin dump (CLI mode)");

        let mut cmd = Command::new(&self.executable);
        cmd.arg("dump");
        cmd.arg(&info.path);
        configure_logging_for_child(&mut cmd);

        // TODO: Pass offset/length when the plugin protocol supports it.

        let output = cmd.output()?;

        if !output.status.success() {
            warn!(plugin = self.info.name, stderr = %String::from_utf8_lossy(&output.stderr), "plugin dump failed");
            return Err(std::io::Error::other(format!(
                "Plugin {} failed: {}",
                self.name(),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        writer.write_all(&output.stdout)?;
        Ok(())
    }
}
