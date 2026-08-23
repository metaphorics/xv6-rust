#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ustd::abi::fcntl::O_RDONLY;

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    if args.len() <= 1 {
        return count(0, b"");
    }
    for path in &args[1..] {
        let fd = ustd::open(path, O_RDONLY);
        if fd < 0 {
            ustd::println!("wc: cannot open {}", text(path));
            return 1;
        }
        if count(fd, path) != 0 {
            let _ = ustd::close(fd);
            return 1;
        }
        let _ = ustd::close(fd);
    }
    0
}

fn count(fd: i32, name: &[u8]) -> i32 {
    let mut lines = 0;
    let mut words = 0;
    let mut bytes = 0;
    let mut in_word = false;
    let mut buffer = [0; 512];
    loop {
        let n = ustd::read(fd, &mut buffer);
        if n == 0 {
            break;
        }
        if n < 0 {
            ustd::println!("wc: read error");
            return 1;
        }
        for byte in &buffer[..n as usize] {
            bytes += 1;
            if *byte == b'\n' {
                lines += 1;
            }
            if matches!(*byte, b' ' | b'\r' | b'\t' | b'\n' | 0x0b) {
                in_word = false;
            } else if !in_word {
                words += 1;
                in_word = true;
            }
        }
    }
    ustd::println!("{lines} {words} {bytes} {}", text(name));
    0
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
