#![no_std]
#![no_main]
#![forbid(unsafe_code)]

ustd::entry!(main);

fn main(_args: &[&[u8]]) -> i32 {
    if ustd::fork() > 0 {
        let _ = ustd::pause(5);
    }
    0
}
