//! rcat — the rust-cat binary
//!
//! A modern, extensible terminal file viewer for text and binary files.

// Phase 0 skeleton code — some clippy lints are intentionally suppressed here.
#![allow(
    clippy::needless_range_loop,   // the hex dump loops are clearer this way for now
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::enum_variant_names     // Shell::Bash etc. are intentionally the canonical names
)]

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

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
                  Examples:\n  rcat README.md\n  rcat --hex /bin/ls\n  rcat --offset 0x1000 --length 256 firmware.bin"
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
enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.list_viewers {
        println!("Built-in viewers (v0.1):");
        println!("  text   - UTF-8 text pager with graceful fallback");
        println!("  hex    - Classic 16-byte hex + ASCII view (color-coded)");
        println!("\nMore viewers will be available via plugins in future releases.");
        return Ok(());
    }

    if let Some(Commands::Completions { shell }) = cli.command {
        // Placeholder — real implementation uses clap_complete
        eprintln!(
            "Completions for {:?} not yet implemented in this build",
            shell
        );
        return Ok(());
    }

    // Parse offset (support 0x and decimal)
    let offset = parse_offset(&cli.offset)?;

    let mode = if cli.hex {
        ViewMode::Hex
    } else if cli.text {
        ViewMode::Text
    } else {
        ViewMode::Auto
    };

    let use_stdout = cli.stdout || !atty::is(atty::Stream::Stdout);

    if let Some(path) = &cli.file {
        if use_stdout {
            // Non-interactive path (Phase 0: simple implementation)
            run_non_interactive(path, mode, offset, cli.length)?;
        } else {
            // Interactive TUI (not yet implemented in Phase 0)
            println!("rcat TUI is not yet wired up (Phase 0 skeleton).");
            println!("File: {}", path.display());
            println!("Mode: {:?}", mode);
            println!("Offset: 0x{:x}", offset);
            println!("\nUse --stdout to force non-interactive output, or wait for the full TUI.");
            println!("Example: rcat --stdout {}", path.display());
        }
    } else {
        // stdin path (future)
        eprintln!("Reading from stdin is not fully supported yet. Please provide a file path.");
        std::process::exit(1);
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Auto,
    Text,
    Hex,
}

fn parse_offset(s: &str) -> anyhow::Result<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| anyhow::anyhow!("invalid hex offset: {}", e))
    } else {
        s.parse::<u64>()
            .map_err(|e| anyhow::anyhow!("invalid offset: {}", e))
    }
}

fn run_non_interactive(
    path: &PathBuf,
    mode: ViewMode,
    offset: u64,
    length: Option<u64>,
) -> anyhow::Result<()> {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Read};

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    // Skip to offset (simple seek for Phase 0; later use mmap)
    if offset > 0 {
        let mut skipped = 0u64;
        let mut buf = [0u8; 8192];
        while skipped < offset {
            let to_read = std::cmp::min((offset - skipped) as usize, buf.len());
            let n = reader.read(&mut buf[..to_read])?;
            if n == 0 {
                break;
            }
            skipped += n as u64;
        }
    }

    let max_bytes = length.unwrap_or(u64::MAX);
    let mut remaining = max_bytes;

    match mode {
        ViewMode::Text | ViewMode::Auto => {
            // Simple text dump (Phase 0). Real version will do proper line handling + encoding.
            let mut line = String::new();
            while remaining > 0 {
                line.clear();
                let n = reader.read_line(&mut line)?;
                if n == 0 {
                    break;
                }
                let to_write = if (line.len() as u64) > remaining {
                    remaining as usize
                } else {
                    line.len()
                };
                print!("{}", &line[..to_write]);
                remaining = remaining.saturating_sub(to_write as u64);
            }
        }
        ViewMode::Hex => {
            // Basic hex + ASCII dump (xxd-like, Phase 0 quality)
            let mut buf = vec![0u8; 16];
            let mut addr = offset;
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }

                print!("{:08x}: ", addr);
                for i in 0..16 {
                    if i < n {
                        print!("{:02x} ", buf[i]);
                    } else {
                        print!("   ");
                    }
                    if i == 7 {
                        print!(" ");
                    }
                }
                print!(" |");
                for i in 0..n {
                    let c = buf[i];
                    let ch = if (0x20..=0x7e).contains(&c) {
                        c as char
                    } else {
                        '.'
                    };
                    print!("{}", ch);
                }
                println!("|");

                addr += n as u64;
                if (n as u64) > remaining {
                    break;
                }
                remaining -= n as u64;
                if remaining == 0 {
                    break;
                }
            }
        }
    }

    Ok(())
}

// Small helper so we don't pull atty as a hard dep yet in Phase 0.
// We can replace with a tiny isatty check or the `atty` crate later.
mod atty {
    pub enum Stream {
        Stdout,
    }
    pub fn is(_: Stream) -> bool {
        // Conservative: assume interactive unless we can prove otherwise.
        // In real code we'd use libc isatty(1) or crossterm.
        true
    }
}
