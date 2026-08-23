#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ustd::abi::fcntl::O_RDONLY;

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    if args.len() <= 1 {
        ustd::println!("usage: grep pattern [file ...]");
        return 1;
    }
    if args.len() == 2 {
        grep(args[1], 0);
        return 0;
    }
    for path in &args[2..] {
        let fd = ustd::open(path, O_RDONLY);
        if fd < 0 {
            ustd::println!("grep: cannot open {}", text(path));
            return 1;
        }
        grep(args[1], fd);
        let _ = ustd::close(fd);
    }
    0
}

fn grep(pattern: &[u8], fd: i32) {
    let mut buffer = [0; 1024];
    let mut used = 0;
    loop {
        let capacity = buffer.len();
        let n = ustd::read(fd, &mut buffer[used..capacity - 1]);
        if n <= 0 {
            break;
        }
        used += n as usize;
        let mut start = 0;
        while let Some(relative) = buffer[start..used].iter().position(|byte| *byte == b'\n') {
            let end = start + relative;
            if matches(pattern, &buffer[start..end]) {
                let _ = ustd::write(1, &buffer[start..=end]);
            }
            start = end + 1;
        }
        if start != 0 {
            buffer.copy_within(start..used, 0);
            used -= start;
        }
        if used == buffer.len() - 1 {
            used = 0;
        }
    }
}

fn matches(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.first() == Some(&b'^') {
        return match_here(&pattern[1..], text);
    }
    (0..=text.len()).any(|start| match_here(pattern, &text[start..]))
}

fn match_here(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if pattern.get(1) == Some(&b'*') {
        return match_star(pattern[0], &pattern[2..], text);
    }
    if pattern == b"$" {
        return text.is_empty();
    }
    !text.is_empty()
        && (pattern[0] == b'.' || pattern[0] == text[0])
        && match_here(&pattern[1..], &text[1..])
}

fn match_star(byte: u8, pattern: &[u8], mut text: &[u8]) -> bool {
    loop {
        if match_here(pattern, text) {
            return true;
        }
        if text.is_empty() || (byte != b'.' && text[0] != byte) {
            return false;
        }
        text = &text[1..];
    }
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
