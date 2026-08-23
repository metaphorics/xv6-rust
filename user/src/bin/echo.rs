#![no_std]
#![no_main]
#![forbid(unsafe_code)]

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    for (index, arg) in args.iter().skip(1).enumerate() {
        if index != 0 {
            ustd::print!(" ");
        }
        if let Ok(text) = core::str::from_utf8(arg) {
            ustd::print!("{text}");
        } else {
            let _ = ustd::write(1, arg);
        }
    }
    ustd::println!();
    0
}
