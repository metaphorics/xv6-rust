#![forbid(unsafe_code)]

//! Build, static verification, image staging, and QEMU shell smoke tests.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(30);
const USER_PROGRAMS: [&str; 19] = [
    "init",
    "sh",
    "echo",
    "ls",
    "cat",
    "grep",
    "kill",
    "ln",
    "mkdir",
    "rm",
    "wc",
    "zombie",
    "stressfs",
    "sync",
    "logstress",
    "forphan",
    "dorphan",
    "grind",
    "forktest",
];
const README_FIRST_LINE: &str =
    "xv6 is a re-implementation of Dennis Ritchie's and Ken Thompson's Unix";

fn main() {
    let mut args = std::env::args();
    let program = args.next();
    match args.next().as_deref() {
        Some("check") if args.next().is_none() => check(),
        Some("run") => {
            let rest: Vec<String> = args.collect();
            let arch = parse_arch(&rest, program.as_deref());
            run(&arch, rest.iter().any(|arg| arg == "--echo-test"));
        }
        Some("test") => {
            let rest: Vec<String> = args.collect();
            let (arch, test) = parse_test_args(&rest, program.as_deref());
            test_xv6(&arch, test);
        }
        _ => usage(program.as_deref()),
    }
}

#[derive(Clone, Copy)]
enum TestKind {
    Quick,
    Full,
    Crash,
    Log,
    Forphan,
    Dorphan,
}

fn parse_arch(args: &[String], program: Option<&str>) -> String {
    if let Some(pos) = args.iter().position(|arg| arg == "--arch")
        && let Some(arch) = args.get(pos + 1)
    {
        return arch.clone();
    }
    let name = program.unwrap_or("cargo xtask");
    eprintln!("{name}: run expects --arch <riscv64|x86_64>");
    std::process::exit(2);
}

fn parse_test_args(args: &[String], program: Option<&str>) -> (String, TestKind) {
    let arch = parse_arch(args, program);
    let mut test = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--arch" {
            index += 2;
            continue;
        }
        if test.is_some() || args[index].starts_with('-') {
            usage(program);
        }
        test = Some(match args[index].as_str() {
            "quick" => TestKind::Quick,
            "full" => TestKind::Full,
            "crash" => TestKind::Crash,
            "log" => TestKind::Log,
            "forphan" => TestKind::Forphan,
            "dorphan" => TestKind::Dorphan,
            _ => usage(program),
        });
        index += 1;
    }
    (arch, test.unwrap_or(TestKind::Full))
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
        "clippy, kernel dev",
        Command::new("cargo")
            .current_dir(root.join("kernel"))
            .args(["clippy", "--", "-D", "warnings"]),
    );
    step(
        "kernel release",
        Command::new("cargo")
            .current_dir(root.join("kernel"))
            .args(["build", "--release"]),
    );
    step(
        "clippy, user dev",
        Command::new("cargo")
            .current_dir(root.join("user"))
            .args(["clippy", "--bins", "--", "-D", "warnings"]),
    );
    step(
        "user release",
        Command::new("cargo")
            .current_dir(root.join("user"))
            .args(["build", "--release", "--bins"]),
    );
    step(
        "clippy, x86_64 kernel dev",
        Command::new("cargo")
            .current_dir(root.join("kernel"))
            .args([
                "clippy",
                "--target",
                "x86_64-unknown-none",
                "--",
                "-D",
                "warnings",
            ]),
    );
    step(
        "x86_64 kernel release",
        Command::new("cargo")
            .current_dir(root.join("kernel"))
            .args(["build", "--release", "--target", "x86_64-unknown-none"]),
    );
    step(
        "clippy, x86_64 user dev",
        Command::new("cargo").current_dir(root.join("user")).args([
            "clippy",
            "--bins",
            "--target",
            "x86_64-unknown-none",
            "--",
            "-D",
            "warnings",
        ]),
    );
    step(
        "x86_64 user release",
        Command::new("cargo").current_dir(root.join("user")).args([
            "build",
            "--release",
            "--bins",
            "--target",
            "x86_64-unknown-none",
        ]),
    );
    step(
        "host unit tests",
        Command::new("cargo")
            .current_dir(root)
            .args(["test", "-p", "abi", "-p", "mkfs"]),
    );
}

fn run(arch: &str, echo_test: bool) {
    require_arch(arch);
    let root = workspace_root();
    let kernel = build_kernel(root, arch);
    let programs = build_user_programs(root, arch, &[]);
    let image = build_fs_image(root, &programs);
    let qemu = require_qemu(arch);
    let image_files = programs.len() + 1;
    if echo_test {
        boot_echo_test(qemu, arch, &kernel, &image);
    } else {
        boot_shell_smoke(qemu, arch, &kernel, &image, image_files);
    }
}

fn test_xv6(arch: &str, test: TestKind) {
    require_arch(arch);
    let root = workspace_root();
    let extra_programs: &[&str] = match test {
        TestKind::Quick | TestKind::Full => &["usertests"],
        _ => &[],
    };
    let harness = TestHarness {
        root,
        arch: arch.to_owned(),
        kernel: build_kernel(root, arch),
        programs: build_user_programs(root, arch, extra_programs),
        qemu: require_qemu(arch),
    };
    match test {
        TestKind::Quick | TestKind::Full => harness.usertests(test),
        TestKind::Crash => {
            harness.log();
            harness.forphan();
            harness.dorphan();
            println!("XTASK: crash recovery tests ok");
        }
        TestKind::Log => harness.log(),
        TestKind::Forphan => harness.forphan(),
        TestKind::Dorphan => harness.dorphan(),
    }
}

fn require_arch(arch: &str) {
    if !matches!(arch, "riscv64" | "x86_64") {
        fail("arch must be riscv64 or x86_64");
    }
}

fn target_triple(arch: &str) -> &'static str {
    match arch {
        "riscv64" => "riscv64gc-unknown-none-elf",
        "x86_64" => "x86_64-unknown-none",
        _ => fail("unsupported architecture"),
    }
}

struct TestHarness {
    root: &'static Path,
    arch: String,
    kernel: PathBuf,
    programs: Vec<PathBuf>,
    qemu: &'static str,
}

impl TestHarness {
    fn fresh_image(&self) -> PathBuf {
        build_fs_image(self.root, &self.programs)
    }

    fn usertests(&self, test: TestKind) {
        let image = self.fresh_image();
        let (command, timeout) = match test {
            TestKind::Quick => (b"usertests -q\n".as_slice(), Duration::from_secs(300)),
            TestKind::Full => (b"usertests\n".as_slice(), Duration::from_secs(600)),
            _ => fail("internal error: invalid usertests mode"),
        };
        let (mut child, mut console) = spawn_qemu(self.qemu, &self.arch, &self.kernel, &image);
        send_after_prompt(&mut child, &mut console, command);
        let matched = console.wait_for_any(
            &mut child,
            &[b"ALL TESTS PASSED", b"FAILED", b"exec usertests failed"],
            timeout,
        );
        if matched != 0 {
            kill(&mut child);
            fail("usertests reported a failure");
        }
        kill(&mut child);
        println!("XTASK: usertests passed");
    }

    fn log(&self) {
        println!("==> log crash recovery");
        for attempt in 1..=5 {
            let image = self.fresh_image();
            let (mut child, mut console) = spawn_qemu(self.qemu, &self.arch, &self.kernel, &image);
            send_after_prompt(&mut child, &mut console, b"logstress f0 f1 f2 f3 f4 f5\n");
            thread::sleep(Duration::from_secs(2));
            kill(&mut child);

            let (mut child, mut console) = spawn_qemu(self.qemu, &self.arch, &self.kernel, &image);
            let recovered = console.wait_for_any(
                &mut child,
                &[b"recovering tail ", b"init: starting sh"],
                TIMEOUT,
            ) == 0;
            if recovered {
                send_after_prompt(&mut child, &mut console, b"ls\n");
                console.wait_for(&mut child, b"\nf5 ");
                kill(&mut child);
                println!("XTASK: log recovery ok (attempt {attempt})");
                return;
            }
            kill(&mut child);
            println!("==> log attempt {attempt} did not leave a committed transaction");
        }
        fail("log recovery did not trigger in 5 attempts");
    }

    fn forphan(&self) {
        self.orphan(b"forphan\n", "forphan");
    }

    fn dorphan(&self) {
        self.orphan(b"dorphan\n", "dorphan");
    }

    fn orphan(&self, command: &[u8], name: &str) {
        println!("==> {name} crash recovery");
        let image = self.fresh_image();
        let (mut child, mut console) = spawn_qemu(self.qemu, &self.arch, &self.kernel, &image);
        send_after_prompt(&mut child, &mut console, command);
        console.wait_for(&mut child, b"wait for kill and reclaim");
        kill(&mut child);

        let (mut child, mut console) = spawn_qemu(self.qemu, &self.arch, &self.kernel, &image);
        console.wait_for(&mut child, b"ireclaim: orphaned inode ");
        kill(&mut child);
        println!("XTASK: {name} recovery ok");
    }
}

fn build_kernel(root: &Path, arch: &str) -> PathBuf {
    println!("==> cargo build --release, kernel ({arch})");
    let executables = cargo_executables(
        Command::new("cargo")
            .current_dir(root.join("kernel"))
            .args([
                "build",
                "--release",
                "--target",
                target_triple(arch),
                "--message-format=json",
            ]),
        "kernel build",
    );
    executables
        .into_iter()
        .find(|path| path.file_name().is_some_and(|name| name == "kernel"))
        .unwrap_or_else(|| fail("cargo did not report a kernel executable"))
}

fn build_user_programs(root: &Path, arch: &str, extra: &[&str]) -> Vec<PathBuf> {
    let names: Vec<&str> = USER_PROGRAMS
        .iter()
        .copied()
        .chain(extra.iter().copied())
        .collect();
    println!("==> cargo build --release, user programs ({arch})");
    let mut command = Command::new("cargo");
    command.current_dir(root.join("user")).args([
        "build",
        "--release",
        "--target",
        target_triple(arch),
        "--message-format=json",
    ]);
    for name in &names {
        command.arg("--bin").arg(name);
    }
    let executables = cargo_executables(&mut command, "user build");
    names
        .iter()
        .map(|name| {
            let source = executables
                .iter()
                .find(|path| path.file_name().is_some_and(|file| file == *name))
                .unwrap_or_else(|| fail(&format!("cargo did not report user binary {name}")));
            let staged = root.join("user").join(format!("_{name}"));
            fs::copy(source, &staged).unwrap_or_else(|err| {
                fail(&format!("could not stage {}: {err}", staged.display()))
            });
            staged
        })
        .collect()
}

fn cargo_executables(command: &mut Command, what: &str) -> Vec<PathBuf> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|err| fail(&format!("could not run cargo: {err}")));
    let mut executables = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(executable) = json_string_field(&line, "executable") {
                executables.push(executable);
            }
        }
    }
    match child.wait() {
        Ok(status) if status.success() => executables,
        Ok(status) => fail(&format!("{what} failed: {status}")),
        Err(err) => fail(&format!("could not wait for cargo: {err}")),
    }
}

fn build_fs_image(root: &Path, programs: &[PathBuf]) -> PathBuf {
    let image = root.join("fs.img");
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args(["run", "--quiet", "-p", "mkfs", "--"])
        .arg(&image)
        .arg(root.join(".references/xv6-riscv/README"))
        .args(programs);
    step("mkfs fs.img", &mut command);
    image
}

fn json_string_field(line: &str, field: &str) -> Option<PathBuf> {
    let marker = format!("\"{field}\":\"");
    let start = line.find(&marker)? + marker.len();
    let bytes = line.as_bytes();
    let mut value = String::new();
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => return Some(PathBuf::from(value)),
            b'\\' if index + 1 < bytes.len() => {
                index += 1;
                match bytes[index] {
                    b'\\' => value.push('\\'),
                    b'"' => value.push('"'),
                    b'n' => value.push('\n'),
                    b'r' => value.push('\r'),
                    b't' => value.push('\t'),
                    other => value.push(other as char),
                }
            }
            byte => value.push(byte as char),
        }
        index += 1;
    }
    None
}

fn require_qemu(arch: &str) -> &'static str {
    let qemu = match arch {
        "riscv64" => "qemu-system-riscv64",
        "x86_64" => "qemu-system-x86_64",
        _ => fail("unsupported architecture"),
    };
    match Command::new(qemu)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => qemu,
        _ => fail(&format!("{qemu} is required")),
    }
}

struct Console {
    rx: mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Console {
    fn wait_for(&mut self, child: &mut Child, expected: &[u8]) {
        let _ = self.wait_for_any(child, &[expected], TIMEOUT);
    }

    fn wait_for_any(&mut self, child: &mut Child, expected: &[&[u8]], timeout: Duration) -> usize {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some((index, at)) = expected
                .iter()
                .enumerate()
                .filter_map(|(index, marker)| {
                    find_bytes(&self.pending, marker).map(|at| (index, at))
                })
                .min_by_key(|(_, at)| *at)
            {
                self.pending.drain(..at + expected[index].len());
                return index;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(remaining) {
                Ok(chunk) => self.pending.extend_from_slice(&chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    kill(child);
                    let markers: Vec<_> = expected
                        .iter()
                        .map(|marker| String::from_utf8_lossy(marker))
                        .collect();
                    fail(&format!("timed out waiting for one of {markers:?}"));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    kill(child);
                    fail("qemu output ended before the expected console text");
                }
            }
        }
    }
}

fn spawn_qemu(qemu: &str, arch: &str, kernel: &Path, image: &Path) -> (Child, Console) {
    let mut command = Command::new(qemu);
    match arch {
        "riscv64" => {
            command.args([
                "-machine",
                "virt",
                "-bios",
                "none",
                "-m",
                "128M",
                "-smp",
                "3",
                "-nographic",
                "-global",
                "virtio-mmio.force-legacy=false",
            ]);
        }
        "x86_64" => {
            command.args([
                "-machine",
                "q35",
                "-m",
                "128M",
                "-smp",
                "1",
                "-display",
                "none",
                "-serial",
                "stdio",
                "-monitor",
                "none",
                "-no-reboot",
            ]);
        }
        _ => fail("unsupported architecture"),
    }
    let device = if arch == "riscv64" {
        "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0"
    } else {
        "virtio-blk-pci,disable-legacy=on,drive=x0"
    };
    let mut child = command
        .arg("-drive")
        .arg(format!("file={},if=none,format=raw,id=x0", image.display()))
        .arg("-device")
        .arg(device)
        .arg("-kernel")
        .arg(kernel)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|err| fail(&format!("could not spawn qemu: {err}")));

    let (tx, rx) = mpsc::channel();
    let mut stdout = child.stdout.take().expect("qemu stdout");
    thread::spawn(move || {
        let mut buffer = [0; 256];
        while let Ok(n) = stdout.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let chunk = buffer[..n].to_vec();
            let mut terminal = std::io::stdout().lock();
            let _ = terminal.write_all(&chunk).and_then(|()| terminal.flush());
            if tx.send(chunk).is_err() {
                break;
            }
        }
    });
    (
        child,
        Console {
            rx,
            pending: Vec::new(),
        },
    )
}

fn boot_echo_test(qemu: &str, arch: &str, kernel: &Path, image: &Path) -> ! {
    let (mut child, mut console) = spawn_qemu(qemu, arch, kernel, image);
    console.wait_for(&mut child, b"init: starting sh");
    command(&mut child, &mut console, b"echo hi\n", b"hi\n");
    console.wait_for(&mut child, b"$ ");
    println!("XTASK: echo ok");
    kill(&mut child);
    std::process::exit(0)
}

fn boot_shell_smoke(qemu: &str, arch: &str, kernel: &Path, image: &Path, image_files: usize) -> ! {
    let (mut child, mut console) = spawn_qemu(qemu, arch, kernel, image);
    console.wait_for(&mut child, b"init: starting sh");
    command(&mut child, &mut console, b"echo hi\n", b"hi\n");

    send_after_prompt(&mut child, &mut console, b"ls\n");
    console.wait_for(&mut child, b"ls");
    console.wait_for(&mut child, b"\n");
    console.wait_for(&mut child, b"README");
    console.wait_for(&mut child, b"init");
    console.wait_for(&mut child, b"sh");

    send_after_prompt(&mut child, &mut console, b"cat README\n");
    console.wait_for(&mut child, b"cat README");
    console.wait_for(&mut child, b"\n");
    console.wait_for(&mut child, README_FIRST_LINE.as_bytes());

    send_after_prompt(&mut child, &mut console, b"ls | wc\n");
    console.wait_for(&mut child, b"ls | wc");
    console.wait_for(&mut child, b"\n");
    let expected_entries = format!("{} ", image_files + 3);
    console.wait_for(&mut child, expected_entries.as_bytes());

    command_done(&mut child, &mut console, b"mkdir d\n");
    command_done(&mut child, &mut console, b"echo x > d/f\n");
    command(&mut child, &mut console, b"cat d/f\n", b"x\n");
    command(
        &mut child,
        &mut console,
        b"rm d\n",
        b"rm: d failed to delete\n",
    );
    command_done(&mut child, &mut console, b"rm d/f\n");
    command_done(&mut child, &mut console, b"rm d\n");
    command(&mut child, &mut console, b"ls d\n", b"ls: cannot open d\n");
    console.wait_for(&mut child, b"$ ");

    println!("XTASK: M7 shell smoke ok");
    kill(&mut child);
    std::process::exit(0)
}

fn command(child: &mut Child, console: &mut Console, input: &[u8], output: &[u8]) {
    send_after_prompt(child, console, input);
    console.wait_for(child, &input[..input.len() - 1]);
    console.wait_for(child, b"\n");
    console.wait_for(child, output);
}

fn command_done(child: &mut Child, console: &mut Console, input: &[u8]) {
    send_after_prompt(child, console, input);
    console.wait_for(child, &input[..input.len() - 1]);
    console.wait_for(child, b"\n");
}

fn send_after_prompt(child: &mut Child, console: &mut Console, input: &[u8]) {
    console.wait_for(child, b"$ ");
    let Some(stdin) = child.stdin.as_mut() else {
        kill(child);
        fail("qemu stdin is not piped");
    };
    stdin
        .write_all(input)
        .and_then(|()| stdin.flush())
        .unwrap_or_else(|err| fail(&format!("could not write qemu stdin: {err}")));
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn step(what: &str, command: &mut Command) {
    println!("==> {what}");
    match command.status() {
        Ok(status) if status.success() => {}
        Ok(status) => fail(&format!("step {what:?} failed: {status}")),
        Err(err) => fail(&format!("could not run step {what:?}: {err}")),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("xtask: {message}");
    std::process::exit(1)
}

fn usage(program: Option<&str>) -> ! {
    let name = program.unwrap_or("cargo xtask");
    eprintln!("usage: {name} check");
    eprintln!("       {name} run --arch <riscv64|x86_64> [--echo-test]");
    eprintln!("       {name} test --arch <riscv64|x86_64> [quick|full|crash|log|forphan|dorphan]");
    std::process::exit(2)
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
}
