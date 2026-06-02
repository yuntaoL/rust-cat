//! Long-lived plugin subprocess for protocol v2 (`--session` mode).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

use crate::probe::DETECTION_READ_LIMIT;
use crate::plugin::{PluginRequest, PluginResponse};
use crate::session::FileSession;
use tracing::{debug, trace, warn};

/// Active line-oriented JSON session with a plugin executable.
pub struct PluginSession {
    executable: PathBuf,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    timeout: Duration,
    open_path: Option<PathBuf>,
}

impl PluginSession {
    /// Spawn the plugin in `--session` mode (process stays alive for multiple requests).
    pub fn spawn(executable: impl AsRef<Path>, timeout: Duration) -> std::io::Result<Self> {
        let executable = executable.as_ref().to_path_buf();
        debug!(path = %executable.display(), "spawning plugin session");

        let mut cmd = Command::new(&executable);
        cmd.arg("--session");
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        super::external_plugin::configure_logging_for_child(&mut cmd);

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");

        Ok(Self {
            executable,
            child,
            stdin,
            stdout: BufReader::new(stdout),
            timeout,
            open_path: None,
        })
    }

    /// Open a file in the plugin session (host sends prefix bytes from mmap).
    pub fn open(&mut self, session: &FileSession) -> std::io::Result<()> {
        if let Some(open) = &self.open_path {
            if open == session.path() {
                return Ok(());
            }
            self.close()?;
        }

        let prefix_len = (session.size() as usize).min(DETECTION_READ_LIMIT);
        let initial_data = session.slice(0, prefix_len).to_vec();

        let request = PluginRequest::Open {
            file_path: session.path().display().to_string(),
            file_size: session.size(),
            preliminary: session.info().detected.clone(),
            initial_data,
        };

        match self.request(&request)? {
            PluginResponse::OpenResult => {
                self.open_path = Some(session.path().to_path_buf());
                debug!(path = %session.path().display(), "plugin session opened file");
                Ok(())
            }
            PluginResponse::Error { message } => Err(std::io::Error::other(message)),
            other => Err(std::io::Error::other(format!(
                "unexpected Open response: {other:?}"
            ))),
        }
    }

    /// Send one request and return the plugin response (handles `NeedReadBytes` via host mmap).
    pub fn request(&mut self, request: &PluginRequest) -> std::io::Result<PluginResponse> {
        self.write_request(request)?;
        let response = self.read_response()?;
        if matches!(response, PluginResponse::NeedReadBytes { .. }) {
            return Err(std::io::Error::other(
                "NeedReadBytes requires request_with_file(session)",
            ));
        }
        Ok(response)
    }

    /// Close the current file in the plugin session.
    pub fn close(&mut self) -> std::io::Result<()> {
        if self.open_path.is_none() {
            return Ok(());
        }
        let _ = self.request(&PluginRequest::Close)?;
        self.open_path = None;
        Ok(())
    }

    /// Send a request; if the plugin returns `NeedReadBytes`, satisfy it from host mmap.
    pub fn request_with_file(
        &mut self,
        request: &PluginRequest,
        session: &FileSession,
    ) -> std::io::Result<PluginResponse> {
        self.write_request(request)?;
        let mut response = self.read_response()?;
        while let PluginResponse::NeedReadBytes { offset, length } = response {
            let data = session.slice(offset, length).to_vec();
            self.write_request(&PluginRequest::ReadBytes { offset, data })?;
            response = self.read_response()?;
        }
        Ok(response)
    }

    fn write_request(&mut self, request: &PluginRequest) -> std::io::Result<()> {
        let json = serde_json::to_string(request).map_err(std::io::Error::other)?;
        trace!(request = %json, "plugin session write");
        writeln!(self.stdin, "{json}")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_response(&mut self) -> std::io::Result<PluginResponse> {
        let deadline = Instant::now() + self.timeout;
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line)? == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!(
                        "plugin session {:?} closed stdout",
                        self.executable.display()
                    ),
                ));
            }
            if !line.trim().is_empty() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "plugin session {:?} timed out waiting for response",
                        self.executable.display()
                    ),
                ));
            }
        }
        serde_json::from_str(line.trim()).map_err(|e| {
            std::io::Error::other(format!(
                "invalid plugin session JSON: {e}; line={line}"
            ))
        })
    }
}

impl Drop for PluginSession {
    fn drop(&mut self) {
        let _ = self.close();
        if let Err(e) = self.child.kill() {
            warn!(error = %e, "failed to kill plugin session child");
        }
        let _ = self.child.wait();
    }
}