#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ustd::abi::fcntl::{O_CREATE, O_RDWR};

const N: usize = 250;
const SZ: usize = 2000;

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    for (index, path) in args[1..].iter().enumerate() {
        let pid = ustd::fork();
        if pid < 0 {
            ustd::println!("{}: fork failed", text(args[0]));
            return 1;
        }
        if pid == 0 {
            let fd = ustd::open(path, O_CREATE | O_RDWR);
            if fd < 0 {
                ustd::println!("{}: create {} failed", text(args[0]), text(path));
                return 1;
            }
            let buffer = [b'1' + index as u8; SZ];
            for _ in 0..N {
                let written = ustd::write(fd, &buffer);
                if written != SZ as isize {
                    ustd::println!("write failed {written}");
                    return 1;
                }
            }
            return 0;
        }
    }
    for _ in &args[1..] {
        let mut status = 0;
        let _ = ustd::wait(Some(&mut status));
        if status != 0 {
            return status;
        }
    }
    0
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
