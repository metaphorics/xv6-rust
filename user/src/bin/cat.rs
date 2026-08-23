#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ustd::abi::fcntl::O_RDONLY;

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    if args.len() == 1 {
        return copy(0);
    }
    let mut status = 0;
    for path in &args[1..] {
        let fd = ustd::open(path, O_RDONLY);
        if fd < 0 {
            ustd::println!("cat: cannot open {}", display(path));
            status = 1;
            continue;
        }
        status |= copy(fd);
        let _ = ustd::close(fd);
    }
    status
}

fn copy(fd: i32) -> i32 {
    let mut buffer = [0; 512];
    loop {
        let n = ustd::read(fd, &mut buffer);
        if n == 0 {
            return 0;
        }
        if n < 0 || ustd::write(1, &buffer[..n as usize]) != n {
            ustd::println!("cat: read/write error");
            return 1;
        }
    }
}

fn display(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
