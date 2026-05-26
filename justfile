# rust-cat justfile
# High-productivity commands for development
# Install just: https://github.com/casey/just

set shell := ["bash", "-cu"]

default:
    @just --list

# --- Build & Check ------------------------------------------------------------
build:
    cargo build --workspace --all-targets

check:
    cargo check --workspace --all-targets --all-features

test:
    cargo test --workspace --all-features

# Fast check + lint (what CI runs)
lint:
    cargo fmt -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

fmt:
    cargo fmt --all

# --- Run ----------------------------------------------------------------------
# Run the binary (forward any args)
run *ARGS:
    cargo run --bin rcat -- {{ARGS}}

# Example: just run README.md
# Example: just run -- --help

# Development run with release for large files
run-release *ARGS:
    cargo run --release --bin rcat -- {{ARGS}}

# --- Quality ------------------------------------------------------------------
clean:
    cargo clean

doc:
    cargo doc --workspace --no-deps --open

# Update dependencies
update:
    cargo update

# Audit (requires cargo-deny and/or cargo-audit installed)
audit:
    cargo deny check 2>/dev/null || echo "cargo-deny not installed"
    cargo audit 2>/dev/null || echo "cargo-audit not installed"

# --- Release prep -------------------------------------------------------------
release-check:
    just lint
    just test
    cargo build --release --bin rcat
    @echo "Release build successful. Tag and push to trigger release workflow."

# Install locally (for testing the installed binary)
install:
    cargo install --path crates/rcat --force

# --- Git helpers --------------------------------------------------------------
commit *MSG:
    git add -A
    git commit -m "{{MSG}}"

amend:
    git add -A
    git commit --amend --no-edit
