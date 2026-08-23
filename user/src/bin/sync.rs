#![no_std]
#![no_main]
#![forbid(unsafe_code)]

ustd::entry!(main);

fn main(_args: &[&[u8]]) -> i32 {
    ustd::sync()
}
