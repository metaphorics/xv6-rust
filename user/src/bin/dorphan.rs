#![no_std]
#![no_main]
#![forbid(unsafe_code)]

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    let name = args.first().copied().unwrap_or(b"dorphan");
    if ustd::mkdir(b"dd") != 0 {
        ustd::println!("{}: mkdir dd failed", text(name));
        return 1;
    }
    if ustd::chdir(b"dd") != 0 {
        ustd::println!("{}: chdir dd failed", text(name));
        return 1;
    }
    if ustd::unlink(b"../dd") < 0 {
        ustd::println!("{}: unlink failed", text(name));
        return 1;
    }
    ustd::println!("wait for kill and reclaim");
    loop {
        let _ = ustd::pause(1000);
    }
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
