#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use ustd::abi::fcntl::{O_CREATE, O_RDONLY, O_TRUNC, O_WRONLY};

const MAXARGS: usize = 10;

ustd::entry!(main);

enum Cmd {
    Exec(Vec<Vec<u8>>),
    Redir {
        cmd: Box<Cmd>,
        file: Vec<u8>,
        mode: i32,
        fd: i32,
    },
    Pipe(Box<Cmd>, Box<Cmd>),
    List(Box<Cmd>, Box<Cmd>),
    Back(Box<Cmd>),
}

fn main(_args: &[&[u8]]) -> i32 {
    while {
        let fd = ustd::open(b"console", ustd::abi::fcntl::O_RDWR);
        if fd >= 3 {
            let _ = ustd::close(fd);
            false
        } else {
            fd >= 0
        }
    } {}

    let mut buffer = [0; 100];
    loop {
        let _ = ustd::write(2, b"$ ");
        let n = ustd::gets(&mut buffer);
        if n == 0 {
            return 0;
        }
        let command = trim_start(&buffer[..n]);
        if command.iter().all(|byte| is_whitespace(*byte)) {
            continue;
        }
        if command.starts_with(b"cd ") {
            // The final line may arrive at EOF without a newline.
            let end = command
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(command.len());
            if ustd::chdir(&command[3..end]) < 0 {
                ustd::println!("cannot cd {}", display(&command[3..end]));
            }
            continue;
        }
        let mut parser = Parser::new(command);
        let cmd = match parser.parse_cmd() {
            Ok(cmd) if parser.finished() => cmd,
            Ok(_) => {
                ustd::println!("syntax: leftovers");
                continue;
            }
            Err(message) => {
                ustd::println!("{message}");
                continue;
            }
        };
        let pid = fork_or_die();
        if pid == 0 {
            run(cmd);
        }
        let _ = ustd::wait(None);
    }
}

fn run(cmd: Cmd) -> ! {
    match cmd {
        Cmd::Exec(argv) => {
            if argv.is_empty() {
                ustd::exit(1);
            }
            let args: Vec<&[u8]> = argv.iter().map(Vec::as_slice).collect();
            let _ = ustd::exec(args[0], &args);
            ustd::println!("exec {} failed", display(args[0]));
        }
        Cmd::Redir {
            cmd,
            file,
            mode,
            fd,
        } => {
            let _ = ustd::close(fd);
            if ustd::open(&file, mode) < 0 {
                ustd::println!("open {} failed", display(&file));
                ustd::exit(1);
            }
            run(*cmd);
        }
        Cmd::List(left, right) => {
            if fork_or_die() == 0 {
                run(*left);
            }
            let _ = ustd::wait(None);
            run(*right);
        }
        Cmd::Pipe(left, right) => {
            let mut pipe = [0; 2];
            if ustd::pipe(&mut pipe) < 0 {
                ustd::println!("pipe failed");
                ustd::exit(1);
            }
            if fork_or_die() == 0 {
                let _ = ustd::close(1);
                let _ = ustd::dup(pipe[1]);
                let _ = ustd::close(pipe[0]);
                let _ = ustd::close(pipe[1]);
                run(*left);
            }
            if fork_or_die() == 0 {
                let _ = ustd::close(0);
                let _ = ustd::dup(pipe[0]);
                let _ = ustd::close(pipe[0]);
                let _ = ustd::close(pipe[1]);
                run(*right);
            }
            let _ = ustd::close(pipe[0]);
            let _ = ustd::close(pipe[1]);
            let _ = ustd::wait(None);
            let _ = ustd::wait(None);
        }
        Cmd::Back(cmd) => {
            if fork_or_die() == 0 {
                run(*cmd);
            }
        }
    }
    ustd::exit(0)
}

fn fork_or_die() -> i32 {
    let pid = ustd::fork();
    if pid < 0 {
        ustd::println!("fork failed");
        ustd::exit(1);
    }
    pid
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    fn parse_cmd(&mut self) -> Result<Cmd, &'static str> {
        self.parse_line()
    }

    fn parse_line(&mut self) -> Result<Cmd, &'static str> {
        let mut cmd = self.parse_pipe()?;
        while self.peek(b"&") {
            self.get_token();
            cmd = Cmd::Back(Box::new(cmd));
        }
        if self.peek(b";") {
            self.get_token();
            cmd = Cmd::List(Box::new(cmd), Box::new(self.parse_line()?));
        }
        Ok(cmd)
    }

    fn parse_pipe(&mut self) -> Result<Cmd, &'static str> {
        let left = self.parse_exec()?;
        if self.peek(b"|") {
            self.get_token();
            Ok(Cmd::Pipe(Box::new(left), Box::new(self.parse_pipe()?)))
        } else {
            Ok(left)
        }
    }

    fn parse_exec(&mut self) -> Result<Cmd, &'static str> {
        if self.peek(b"(") {
            self.get_token();
            let cmd = self.parse_line()?;
            if !self.peek(b")") {
                return Err("syntax: missing )");
            }
            self.get_token();
            return self.parse_redirs(cmd);
        }

        let mut argv = Vec::new();
        let mut redirs = Vec::new();
        self.collect_redirs(&mut redirs)?;
        while !self.peek(b"|)&;") {
            let (token, start, end) = self.get_token();
            if token == 0 {
                break;
            }
            if token != b'a' {
                return Err("syntax");
            }
            if argv.len() + 1 >= MAXARGS {
                return Err("too many args");
            }
            argv.push(self.input[start..end].to_vec());
            self.collect_redirs(&mut redirs)?;
        }
        let mut cmd = Cmd::Exec(argv);
        for (file, mode, fd) in redirs {
            cmd = Cmd::Redir {
                cmd: Box::new(cmd),
                file,
                mode,
                fd,
            };
        }
        Ok(cmd)
    }

    fn parse_redirs(&mut self, mut cmd: Cmd) -> Result<Cmd, &'static str> {
        let mut redirs = Vec::new();
        self.collect_redirs(&mut redirs)?;
        for (file, mode, fd) in redirs {
            cmd = Cmd::Redir {
                cmd: Box::new(cmd),
                file,
                mode,
                fd,
            };
        }
        Ok(cmd)
    }

    fn collect_redirs(&mut self, out: &mut Vec<(Vec<u8>, i32, i32)>) -> Result<(), &'static str> {
        while self.peek(b"<>") {
            let (token, _, _) = self.get_token();
            let (word, start, end) = self.get_token();
            if word != b'a' {
                return Err("missing file for redirection");
            }
            let (mode, fd) = match token {
                b'<' => (O_RDONLY, 0),
                b'>' => (O_WRONLY | O_CREATE | O_TRUNC, 1),
                b'+' => (O_WRONLY | O_CREATE, 1),
                _ => return Err("bad redirection"),
            };
            out.push((self.input[start..end].to_vec(), mode, fd));
        }
        Ok(())
    }

    fn get_token(&mut self) -> (u8, usize, usize) {
        self.skip_whitespace();
        let start = self.pos;
        if self.pos == self.input.len() {
            return (0, start, start);
        }
        let byte = self.input[self.pos];
        let token = match byte {
            b'|' | b'(' | b')' | b';' | b'&' | b'<' => {
                self.pos += 1;
                byte
            }
            b'>' => {
                self.pos += 1;
                if self.input.get(self.pos) == Some(&b'>') {
                    self.pos += 1;
                    b'+'
                } else {
                    b'>'
                }
            }
            _ => {
                while self.pos < self.input.len()
                    && !is_whitespace(self.input[self.pos])
                    && !b"<|>&;()".contains(&self.input[self.pos])
                {
                    self.pos += 1;
                }
                b'a'
            }
        };
        let end = self.pos;
        self.skip_whitespace();
        (token, start, end)
    }

    fn peek(&mut self, tokens: &[u8]) -> bool {
        self.skip_whitespace();
        self.input
            .get(self.pos)
            .is_some_and(|byte| tokens.contains(byte))
    }

    fn finished(&mut self) -> bool {
        self.skip_whitespace();
        self.pos == self.input.len()
    }

    fn skip_whitespace(&mut self) {
        while self
            .input
            .get(self.pos)
            .is_some_and(|byte| is_whitespace(*byte))
        {
            self.pos += 1;
        }
    }
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b)
}

fn trim_start(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|byte| is_whitespace(*byte)) {
        bytes = &bytes[1..];
    }
    bytes
}

fn display(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
