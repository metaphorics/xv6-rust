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
//!
//! `cargo xtask run --arch riscv64` builds the kernel, boots it under QEMU
//! (`virt`, `-bios none`), and reports success when the boot banner
//! appears on the serial console.

use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How long `run` waits for the expected banner before killing QEMU.
const BOOT_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
    let mut args = std::env::args();
    let program = args.next();
    let subcommand = args.next();
    let rest: Vec<String> = args.collect();
    match subcommand.as_deref() {
        Some("check") if rest.is_empty() => check(),
        Some("run") => run(&parse_arch(&rest, program.as_deref())),
        _ => usage(program.as_deref()),
    }
}

fn parse_arch(args: &[String], program: Option<&str>) -> String {
    if let [flag, arch] = args
        && flag == "--arch"
    {
        return arch.clone();
    }
    let name = program.unwrap_or("cargo xtask");
    eprintln!("{name}: 'run' expects exactly one '--arch <riscv64>' argument");
    std::process::exit(2);
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

/// Build the kernel for its default target and boot it in QEMU until the
/// expected banner appears on the serial console.
fn run(arch: &str) {
    if arch != "riscv64" {
        eprintln!(
            "xtask: --arch {arch} is not bootable yet; only riscv64 is \
             implemented (x86_64 arrives with its adapter)"
        );
        std::process::exit(1);
    }
    let root = workspace_root();
    let kernel = build_kernel(root);
    let qemu = require_qemu();
    boot_and_expect(qemu, &kernel, "xv6-rust kernel is booting");
}

/// Build the kernel and return its executable path.
///
/// The artifact path is taken from cargo's `--message-format=json`
/// compiler-artifact records rather than guessed, so a `build-dir` /
/// `target-dir` override cannot send QEMU at a stale ELF.
fn build_kernel(root: &Path) -> PathBuf {
    println!("==> cargo build, kernel");
    let mut child = match Command::new("cargo")
        .current_dir(root.join("kernel"))
        .args(["build", "--message-format=json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!("xtask: could not run cargo: {err}");
            std::process::exit(1);
        }
    };
    let mut kernel = None;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { continue };
            if let Some(executable) = json_string_field(&line, "executable")
                && executable.file_name().is_some_and(|name| name == "kernel")
            {
                kernel = Some(executable);
            }
        }
    }
    match child.wait() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("xtask: kernel build failed: {status}");
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("xtask: could not wait for cargo: {err}");
            std::process::exit(1);
        }
    }
    match kernel {
        Some(kernel) => kernel,
        None => {
            eprintln!("xtask: cargo did not report a kernel executable");
            std::process::exit(1);
        }
    }
}

/// Extract a `"field":"value"` string from one JSON line, or `None` when
/// the field is absent or null. Cargo's messages are emitted by serde_json
/// with stable member ordering per record type, so this scan is exact
/// enough for the artifact records read here.
fn json_string_field(line: &str, field: &str) -> Option<PathBuf> {
    let needle = format!("\"{field}\":\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let mut end = None;
    let mut escapes = 0;
    for (i, b) in rest.bytes().enumerate() {
        if b == b'\\' {
            escapes += 1;
        } else if b == b'"' && escapes % 2 == 0 {
            end = Some(i);
            break;
        } else {
            escapes = 0;
        }
    }
    let end = end?;
    let value = &rest[..end];
    if value.contains("\\u{") {
        // Rust-style escaped path (never emitted for cargo artifact paths).
        return None;
    }
    Some(PathBuf::from(
        value.replace("\\\\", "\\").replace("\\\"", "\""),
    ))
}

/// Fail with an installation hint when QEMU is missing.
fn require_qemu() -> &'static str {
    let qemu = "qemu-system-riscv64";
    let found = Command::new(qemu)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !found {
        eprintln!("xtask: '{qemu}' not found on PATH");
        eprintln!("       install it first, e.g.: sudo apt-get install -y qemu-system-riscv");
        std::process::exit(1);
    }
    qemu
}

/// Boot `kernel` under QEMU on the `virt` machine and wait for `expect` to
/// appear on the serial console. Prints QEMU's output as it arrives; exits
/// 0 on match, 1 on timeout or early exit.
fn boot_and_expect(qemu: &str, kernel: &Path, expect: &str) -> ! {
    let mut child = match Command::new(qemu)
        .args([
            "-machine",
            "virt",
            "-bios",
            "none",
            "-m",
            "128M",
            "-smp",
            "3",
            "-nographic",
            "-kernel",
        ])
        .arg(kernel)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!("xtask: could not spawn {qemu}: {err}");
            std::process::exit(1);
        }
    };

    // Echo QEMU's serial output while forwarding each line to the matcher.
    // The echo deliberately avoids `println!`: if the invoking side closes
    // our stdout, a panicking print would silently kill this thread and
    // take the matcher down with it.
    let (tx, rx) = mpsc::channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        let mut out = std::io::stdout().lock();
                        let _ = writeln!(out, "{line}");
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        // `saturating_duration_since` so an expired deadline simply turns
        // into an immediate `Timeout` below instead of panicking.
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(line) if line.contains(expect) => {
                println!("XTASK: ok");
                kill(&mut child);
                std::process::exit(0);
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                kill(&mut child);
                eprintln!(
                    "xtask: timed out after {} s waiting for '{expect}'",
                    BOOT_TIMEOUT.as_secs()
                );
                std::process::exit(1);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The reader thread is gone: either QEMU exited or our
                // own stdout was closed. Kill rather than wait, so a
                // still-running QEMU cannot wedge the harness.
                kill(&mut child);
                eprintln!("xtask: qemu output ended before printing '{expect}'");
                std::process::exit(1);
            }
        }
    }
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
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
    eprintln!("       {name} run --arch <riscv64>");
    eprintln!();
    eprintln!("subcommands:");
    eprintln!("  check  rustfmt, clippy -D warnings (host + riscv64gc targets), host tests");
    eprintln!("  run    build the kernel and boot it in QEMU until the banner appears");
    std::process::exit(2);
}

/// The workspace root, derived from this crate's location so the steps work
/// regardless of the invocation directory.
fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
}
