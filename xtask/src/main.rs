#![forbid(unsafe_code)]

//! Workspace orchestration for xv6-rust.
//!
//! `cargo xtask check` runs the full static gate: rustfmt, clippy with
//! `-D warnings` for the host members and for the kernel and user crates on
//! their default `riscv64gc-unknown-none-elf` target, then the host unit
//! tests. The kernel and user crates configure their build target in their
//! own `.cargo/config.toml`, which cargo discovers from the working
//! directory of the invocation rather than from the manifest, so those two
//! checks are spawned with the crate directory as the working directory.

use std::path::Path;
use std::process::Command;

fn main() {
    let mut args = std::env::args();
    let program = args.next();
    let subcommand = args.next();
    let extra = args.next();
    match (&subcommand, extra) {
        (Some(sub), None) if sub == "check" => check(),
        _ => usage(program.as_deref()),
    }
}

fn check() {
    let root = workspace_root();
    step(
        "rustfmt --check",
        Command::new("cargo")
            .current_dir(root)
            .args(["fmt", "--all", "--check"]),
    );
    step(
        "clippy, host members",
        Command::new("cargo").current_dir(root).args([
            "clippy",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
    );
    step(
        "clippy, kernel on riscv64gc-unknown-none-elf",
        Command::new("cargo")
            .current_dir(root.join("kernel"))
            .args(["clippy", "--", "-D", "warnings"]),
    );
    step(
        "clippy, user on riscv64gc-unknown-none-elf",
        Command::new("cargo")
            .current_dir(root.join("user"))
            .args(["clippy", "--", "-D", "warnings"]),
    );
    step(
        "host unit tests",
        Command::new("cargo")
            .current_dir(root)
            .args(["test", "-p", "abi", "-p", "mkfs"]),
    );
}

/// Run one step of the gate, exiting non-zero with a clear message on
/// failure.
fn step(what: &str, cmd: &mut Command) {
    println!("==> {what}");
    match cmd.status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("xtask: step '{what}' failed: {status}");
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("xtask: could not run step '{what}': {err}");
            std::process::exit(1);
        }
    }
}

fn usage(program: Option<&str>) -> ! {
    let name = program.unwrap_or("cargo xtask");
    eprintln!("usage: {name} <check>");
    eprintln!();
    eprintln!("subcommands:");
    eprintln!("  check  rustfmt, clippy -D warnings (host + riscv64gc targets), host tests");
    std::process::exit(2);
}

/// The workspace root, derived from this crate's location so the steps work
/// regardless of the invocation directory.
fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
}
