#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ustd::abi::fcntl::{O_CREATE, O_RDONLY, O_RDWR};

ustd::entry!(main);

fn main(_args: &[&[u8]]) -> i32 {
    ustd::println!("stressfs starting");
    let mut data = [b'a'; 512];
    let mut child = 0u8;
    for index in 0..4 {
        child = index;
        if ustd::fork() > 0 {
            break;
        }
    }
    ustd::println!("write {child}");
    let mut path = *b"stressfs0";
    path[8] += child;
    let fd = ustd::open(&path, O_CREATE | O_RDWR);
    for _ in 0..20 {
        let _ = ustd::write(fd, &data);
    }
    let _ = ustd::close(fd);

    ustd::println!("read");
    let fd = ustd::open(&path, O_RDONLY);
    for _ in 0..20 {
        let _ = ustd::read(fd, &mut data);
    }
    let _ = ustd::close(fd);
    let _ = ustd::wait(None);
    0
}
