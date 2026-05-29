//! Discovery of external command plugins.

use std::path::{Path, PathBuf};
use tracing::{debug, trace};

/// Returns possible plugin directories to search, in order of priority.
pub fn plugin_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. Same directory as the current executable (great for development)
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(dir) = exe_path.parent()
    {
        debug!(path = %dir.display(), "adding executable dir to plugin search paths");
        paths.push(dir.to_path_buf());
    }

    // 2. User config directory: ~/.config/rcat/plugins/
    if let Some(config_dir) = dirs::config_dir() {
        let plugin_dir = config_dir.join("rcat").join("plugins");
        debug!(path = %plugin_dir.display(), "adding user config dir to plugin search paths");
        paths.push(plugin_dir);
    }

    trace!(count = paths.len(), "plugin search paths computed");
    paths
}

/// Discovers potential plugin executables in the given directories.
/// Looks for files starting with "rcat-viewer-" or "rcat-plugin-".
pub fn discover_plugins(search_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut plugins = Vec::new();

    for dir in search_paths {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_potential_plugin(&path) {
                    // On Unix, also check if it's executable
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(metadata) = entry.metadata() {
                            let mode = metadata.permissions().mode();
                            if mode & 0o111 != 0 {
                                debug!(path = %path.display(), "discovered executable plugin");
                                plugins.push(path);
                            } else {
                                trace!(path = %path.display(), "skipping non-executable potential plugin");
                            }
                        }
                    }

                    #[cfg(not(unix))]
                    {
                        debug!(path = %path.display(), "discovered potential plugin (non-unix)");
                        plugins.push(path);
                    }
                }
            }
        }
    }

    debug!(count = plugins.len(), "plugin discovery complete");
    plugins
}

fn is_potential_plugin(path: &Path) -> bool {
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        file_name.starts_with("rcat-viewer-") || file_name.starts_with("rcat-plugin-")
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_potential_plugins_by_name() {
        assert!(is_potential_plugin(Path::new("rcat-viewer-elf")));
        assert!(is_potential_plugin(Path::new("rcat-plugin-png")));
        assert!(!is_potential_plugin(Path::new("some-random-binary")));
    }

    #[test]
    fn plugin_search_paths_returns_at_least_executable_dir() {
        let paths = plugin_search_paths();
        // Should at least contain the directory of the current test binary
        assert!(!paths.is_empty());
    }
}
