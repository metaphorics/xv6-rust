#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ustd::abi::fcntl::O_RDWR;

ustd::entry!(main);

fn main(_args: &[&[u8]]) -> i32 {
    if ustd::open(b"console", O_RDWR) < 0 {
        let _ = ustd::mknod(b"console", 1, 0);
        if ustd::open(b"console", O_RDWR) < 0 {
            return 1;
        }
    }
    let _ = ustd::dup(0);
    let _ = ustd::dup(0);

    loop {
        ustd::println!("init: starting sh");
        let pid = ustd::fork();
        if pid < 0 {
            ustd::println!("init: fork failed");
            continue;
        }
        if pid == 0 {
            let _ = ustd::exec(b"sh", &[b"sh"]);
            ustd::println!("init: exec sh failed");
            return 1;
        }
        loop {
            let child = ustd::wait(None);
            if child == pid || child < 0 {
                break;
            }
        }
    }
}
