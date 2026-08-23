#![no_std]
#![no_main]
#![forbid(unsafe_code)]

const N: usize = 1000;

ustd::entry!(main);

fn main(_args: &[&[u8]]) -> i32 {
    ustd::print!("fork test\n");
    let mut created = 0;
    while created < N {
        let pid = ustd::fork();
        if pid < 0 {
            break;
        }
        if pid == 0 {
            return 0;
        }
        created += 1;
    }
    if created == N {
        ustd::print!("fork claimed to work N times!\n");
        return 1;
    }
    while created > 0 {
        if ustd::wait(None) < 0 {
            ustd::print!("wait stopped early\n");
            return 1;
        }
        created -= 1;
    }
    if ustd::wait(None) != -1 {
        ustd::print!("wait got too many\n");
        return 1;
    }
    ustd::print!("fork test OK\n");
    0
}
