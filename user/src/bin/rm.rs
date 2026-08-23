#![no_std]
#![no_main]
#![forbid(unsafe_code)]

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    if args.len() < 2 {
        ustd::println!("Usage: rm files...");
        return 1;
    }
    for path in &args[1..] {
        if ustd::unlink(path) < 0 {
            ustd::println!("rm: {} failed to delete", text(path));
            break;
        }
    }
    0
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
