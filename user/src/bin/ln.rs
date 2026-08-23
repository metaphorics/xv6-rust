#![no_std]
#![no_main]
#![forbid(unsafe_code)]

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    if args.len() != 3 {
        ustd::println!("Usage: ln old new");
        return 1;
    }
    if ustd::link(args[1], args[2]) < 0 {
        ustd::println!("link {} {}: failed", text(args[1]), text(args[2]));
    }
    0
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
