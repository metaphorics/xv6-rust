#![no_std]
#![no_main]
#![forbid(unsafe_code)]

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    if args.len() < 2 {
        ustd::println!("usage: kill pid...");
        return 1;
    }
    for pid in &args[1..] {
        let _ = ustd::kill(ustd::atoi(pid));
    }
    0
}
