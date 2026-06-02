//! Protocol v2 session state for `rcat-viewer-json --session`.

use std::io;
use std::path::PathBuf;

use rcat_core::FileInfo;
use rcat_core::FileViewer;
use rcat_core::plugin::{PluginRequest, PluginResponse};
use rcat_core::view::ViewContext;
use rcat_viewers_json::JsonViewerLogic;

/// Open file state held for the lifetime of a v2 session.
pub struct PluginSessionState {
    #[allow(dead_code)]
    pub file_path: PathBuf,
    pub info: FileInfo,
    /// Bytes supplied by the host via `ReadBytes` (extends beyond `initial_data` in Open).
    pub host_bytes: Vec<u8>,
}

pub fn handle_session_request(
    state: &mut Option<PluginSessionState>,
    request: &PluginRequest,
) -> io::Result<PluginResponse> {
    let logic = JsonViewerLogic;

    Ok(match request {
        PluginRequest::Open {
            file_path,
            file_size: _,
            preliminary: _,
            initial_data,
        } => {
            let info = FileInfo::from_path(file_path)?;
            *state = Some(PluginSessionState {
                file_path: PathBuf::from(file_path),
                info,
                host_bytes: initial_data.clone(),
            });
            PluginResponse::OpenResult
        }

        PluginRequest::Close => {
            *state = None;
            PluginResponse::CloseResult
        }

        PluginRequest::ReadBytes { offset, data } => {
            let st = state.as_mut().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "no open file in session")
            })?;
            let end = offset.saturating_add(data.len() as u64);
            if end > st.host_bytes.len() as u64 {
                st.host_bytes.resize(end as usize, 0);
            }
            let start = *offset as usize;
            st.host_bytes[start..start + data.len()].copy_from_slice(data);
            PluginResponse::ReadBytesResult
        }

        PluginRequest::RenderViewport {
            start_offset,
            max_rows,
            width,
        } => {
            let st = state.as_ref().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "no open file in session")
            })?;
            let session = rcat_core::FileSession::from_info(st.info.clone())?;
            let ctx = ViewContext::at_byte(&session, *start_offset, *width, *max_rows);
            let vp = logic.render_viewport(&ctx);
            PluginResponse::RenderViewportResult {
                lines: vp.lines,
                status: vp.status,
                source_byte: vp.source_byte,
            }
        }

        PluginRequest::RenderLines {
            start_offset,
            max_rows,
            width,
            ..
        } => {
            let st = session_state(state)?;
            let lines = logic.render_lines(&st.info, *start_offset, *max_rows, *width);
            PluginResponse::RenderLinesResult { lines }
        }

        PluginRequest::AdvanceLines {
            current,
            delta,
            width,
            ..
        } => {
            let st = session_state(state)?;
            let position = logic.advance_lines(&st.info, *current, *delta, *width);
            PluginResponse::AdvanceLinesResult { position }
        }

        PluginRequest::Status { position, .. } => {
            let st = session_state(state)?;
            let status = logic.status(&st.info, *position);
            PluginResponse::StatusResult { status }
        }

        PluginRequest::Dump { offset, length, .. } => {
            let st = session_state(state)?;
            let opts = rcat_core::dump::DumpOptions {
                offset: *offset,
                length: *length,
            };
            let mut buf = Vec::new();
            logic.dump(&st.info, &mut buf, &opts)?;
            PluginResponse::DumpResult {
                output: String::from_utf8_lossy(&buf).into_owned(),
            }
        }

        PluginRequest::CanHandle { .. }
        | PluginRequest::ByteAtDisplayLine { .. }
        | PluginRequest::DisplayLineAtByte { .. } => PluginResponse::Error {
            message: "request not valid in --session mode (use one-shot process)".to_string(),
        },
    })
}

fn session_state(state: &Option<PluginSessionState>) -> io::Result<&PluginSessionState> {
    state
        .as_ref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no open file in session"))
}