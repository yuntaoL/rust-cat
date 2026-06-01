//! Panic-safe terminal lifecycle (raw mode + alternate screen).

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io::{self, stdout};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static TERMINAL_ACTIVE: AtomicBool = AtomicBool::new(false);

fn install_panic_hook() {
    static HOOK: OnceLock<()> = OnceLock::new();
    HOOK.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            restore_terminal();
            previous(panic_info);
        }));
    });
}

/// Restore the terminal to normal mode if we took it over.
pub fn restore_terminal() {
    if TERMINAL_ACTIVE.swap(false, Ordering::SeqCst) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        let _ = execute!(stdout(), crossterm::cursor::Show);
    }
}

/// RAII guard: enables raw mode + alternate screen, restores on drop or panic.
pub struct TerminalGuard;

impl TerminalGuard {
    /// Enter the TUI terminal state. Returns an error if setup fails.
    pub fn enter() -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, crossterm::cursor::Hide)?;
        TERMINAL_ACTIVE.store(true, Ordering::SeqCst);
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}
