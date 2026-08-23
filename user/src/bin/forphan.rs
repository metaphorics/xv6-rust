#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ustd::abi::fcntl::{O_CREATE, O_RDONLY, O_WRONLY};

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    let name = args.first().copied().unwrap_or(b"forphan");
    let path = b"file0";
    let fd = ustd::open(path, O_CREATE | O_WRONLY);
    if fd < 0 {
        ustd::println!("{}: open failed", text(name));
        return 1;
    }
    let Ok(stat) = ustd::fstat(fd) else {
        ustd::println!("{}: cannot stat ff", text(name));
        return 1;
    };
    if ustd::unlink(path) < 0 {
        ustd::println!("{}: unlink failed", text(name));
        return 1;
    }
    if ustd::open(path, O_RDONLY) != -1 {
        ustd::println!("{}: open successed", text(name));
        return 1;
    }
    ustd::println!("wait for kill and reclaim {}", stat.ino);
    loop {
        let _ = ustd::pause(1000);
    }
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
