#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ustd::abi::fcntl::{O_CREATE, O_RDWR};

ustd::entry!(main);

fn main(_args: &[&[u8]]) -> i32 {
    let mut seed = 1u64;
    loop {
        let pid = ustd::fork();
        if pid == 0 {
            return iteration(seed);
        }
        if pid > 0 {
            let _ = ustd::wait(None);
        }
        let _ = ustd::pause(20);
        seed = seed.wrapping_add(1);
    }
}

fn iteration(seed: u64) -> i32 {
    let _ = ustd::unlink(b"a");
    let _ = ustd::unlink(b"b");
    let first = ustd::fork();
    if first < 0 {
        ustd::println!("grind: fork failed");
        return 1;
    }
    if first == 0 {
        return go(seed ^ 31, false);
    }
    let second = ustd::fork();
    if second < 0 {
        ustd::println!("grind: fork failed");
        return 1;
    }
    if second == 0 {
        return go(seed ^ 7177, true);
    }
    let mut first_status = -1;
    let _ = ustd::wait(Some(&mut first_status));
    if first_status != 0 {
        let _ = ustd::kill(first);
        let _ = ustd::kill(second);
    }
    let mut second_status = -1;
    let _ = ustd::wait(Some(&mut second_status));
    0
}

fn go(mut seed: u64, second: bool) -> i32 {
    let mut fd = -1;
    let mut buffer = [0; 999];
    let initial_break = ustd::sbrk(0);
    let mut iterations = 0u64;
    let _ = ustd::mkdir(b"grindir");
    if ustd::chdir(b"grindir") != 0 {
        ustd::println!("grind: chdir grindir failed");
        return 1;
    }
    let _ = ustd::chdir(b"/");

    loop {
        iterations += 1;
        if iterations.is_multiple_of(500) {
            let _ = ustd::write(1, if second { b"B" } else { b"A" });
        }
        match random(&mut seed) % 23 {
            1 => close_open(b"grindir/../a"),
            2 => close_open(b"grindir/../grindir/../b"),
            3 => {
                let _ = ustd::unlink(b"grindir/../a");
            }
            4 => {
                if ustd::chdir(b"grindir") != 0 {
                    ustd::println!("grind: chdir grindir failed");
                    return 1;
                }
                let _ = ustd::unlink(b"../b");
                let _ = ustd::chdir(b"/");
            }
            5 => {
                let _ = ustd::close(fd);
                fd = ustd::open(b"/grindir/../a", O_CREATE | O_RDWR);
            }
            6 => {
                let _ = ustd::close(fd);
                fd = ustd::open(b"/./grindir/./../b", O_CREATE | O_RDWR);
            }
            7 => {
                let _ = ustd::write(fd, &buffer);
            }
            8 => {
                let _ = ustd::read(fd, &mut buffer);
            }
            9 => {
                let _ = ustd::mkdir(b"grindir/../a");
                close_open(b"a/../a/./a");
                let _ = ustd::unlink(b"a/a");
            }
            10 => {
                let _ = ustd::mkdir(b"/../b");
                close_open(b"grindir/../b/b");
                let _ = ustd::unlink(b"b/b");
            }
            11 => {
                let _ = ustd::unlink(b"b");
                let _ = ustd::link(b"../grindir/./../a", b"../b");
            }
            12 => {
                let _ = ustd::unlink(b"../grindir/../a");
                let _ = ustd::link(b".././b", b"/grindir/../a");
            }
            13 => {
                let pid = ustd::fork();
                if pid == 0 {
                    return 0;
                }
                if pid < 0 {
                    ustd::println!("grind: fork failed");
                    return 1;
                }
                let _ = ustd::wait(None);
            }
            14 => {
                let pid = ustd::fork();
                if pid == 0 {
                    let _ = ustd::fork();
                    let _ = ustd::fork();
                    return 0;
                }
                if pid < 0 {
                    ustd::println!("grind: fork failed");
                    return 1;
                }
                let _ = ustd::wait(None);
            }
            15 => {
                let _ = ustd::sbrk(6011);
            }
            16 => {
                let current = ustd::sbrk(0);
                if current > initial_break {
                    let _ = ustd::sbrk(-(current - initial_break));
                }
            }
            17 => {
                let pid = ustd::fork();
                if pid == 0 {
                    close_open(b"a");
                    return 0;
                }
                if pid < 0 {
                    ustd::println!("grind: fork failed");
                    return 1;
                }
                if ustd::chdir(b"../grindir/..") != 0 {
                    ustd::println!("grind: chdir failed");
                    return 1;
                }
                let _ = ustd::kill(pid);
                let _ = ustd::wait(None);
            }
            18 => {
                let pid = ustd::fork();
                if pid == 0 {
                    let _ = ustd::kill(ustd::getpid());
                    return 0;
                }
                if pid < 0 {
                    ustd::println!("grind: fork failed");
                    return 1;
                }
                let _ = ustd::wait(None);
            }
            19 => {
                let mut fds = [0; 2];
                if ustd::pipe(&mut fds) < 0 {
                    ustd::println!("grind: pipe failed");
                    return 1;
                }
                let pid = ustd::fork();
                if pid == 0 {
                    let _ = ustd::fork();
                    let _ = ustd::fork();
                    if ustd::write(fds[1], b"x") != 1 {
                        ustd::println!("grind: pipe write failed");
                    }
                    let mut byte = [0];
                    if ustd::read(fds[0], &mut byte) != 1 {
                        ustd::println!("grind: pipe read failed");
                    }
                    return 0;
                }
                if pid < 0 {
                    ustd::println!("grind: fork failed");
                    return 1;
                }
                let _ = ustd::close(fds[0]);
                let _ = ustd::close(fds[1]);
                let _ = ustd::wait(None);
            }
            20 => {
                let pid = ustd::fork();
                if pid == 0 {
                    let _ = ustd::unlink(b"a");
                    let _ = ustd::mkdir(b"a");
                    let _ = ustd::chdir(b"a");
                    let _ = ustd::unlink(b"../a");
                    let _orphan_fd = ustd::open(b"x", O_CREATE | O_RDWR);
                    let _ = ustd::unlink(b"x");
                    return 0;
                }
                if pid < 0 {
                    ustd::println!("grind: fork failed");
                    return 1;
                }
                let _ = ustd::wait(None);
            }
            21 => match check_resources() {
                0 => {}
                status => return status,
            },
            22 => match check_pipeline() {
                0 => {}
                status => return status,
            },
            _ => {}
        }
    }
}

fn close_open(path: &[u8]) {
    let fd = ustd::open(path, O_CREATE | O_RDWR);
    let _ = ustd::close(fd);
}

fn check_resources() -> i32 {
    let _ = ustd::unlink(b"c");
    let fd = ustd::open(b"c", O_CREATE | O_RDWR);
    if fd < 0 {
        ustd::println!("grind: create c failed");
        return 1;
    }
    if ustd::write(fd, b"x") != 1 {
        ustd::println!("grind: write c failed");
        return 1;
    }
    let Ok(stat) = ustd::fstat(fd) else {
        ustd::println!("grind: fstat failed");
        return 1;
    };
    if stat.size != 1 || stat.ino > 200 {
        ustd::println!("grind: fstat reports invalid file");
        return 1;
    }
    let _ = ustd::close(fd);
    let _ = ustd::unlink(b"c");
    0
}

fn check_pipeline() -> i32 {
    let mut aa = [0; 2];
    let mut bb = [0; 2];
    if ustd::pipe(&mut aa) < 0 || ustd::pipe(&mut bb) < 0 {
        ustd::println!("grind: pipe failed");
        return 1;
    }
    let first = ustd::fork();
    if first == 0 {
        let _ = ustd::close(bb[0]);
        let _ = ustd::close(bb[1]);
        let _ = ustd::close(aa[0]);
        let _ = ustd::close(1);
        if ustd::dup(aa[1]) != 1 {
            return 1;
        }
        let _ = ustd::close(aa[1]);
        let _ = ustd::exec(b"grindir/../echo", &[b"echo", b"hi"]);
        return 2;
    }
    if first < 0 {
        return 3;
    }
    let second = ustd::fork();
    if second == 0 {
        let _ = ustd::close(aa[1]);
        let _ = ustd::close(bb[0]);
        let _ = ustd::close(0);
        if ustd::dup(aa[0]) != 0 {
            return 4;
        }
        let _ = ustd::close(aa[0]);
        let _ = ustd::close(1);
        if ustd::dup(bb[1]) != 1 {
            return 5;
        }
        let _ = ustd::close(bb[1]);
        let _ = ustd::exec(b"/cat", &[b"cat"]);
        return 6;
    }
    if second < 0 {
        return 7;
    }
    let _ = ustd::close(aa[0]);
    let _ = ustd::close(aa[1]);
    let _ = ustd::close(bb[1]);
    let mut output = [0; 3];
    for byte in &mut output {
        let _ = ustd::read(bb[0], core::slice::from_mut(byte));
    }
    let _ = ustd::close(bb[0]);
    let mut first_status = -1;
    let mut second_status = -1;
    let _ = ustd::wait(Some(&mut first_status));
    let _ = ustd::wait(Some(&mut second_status));
    (first_status != 0 || second_status != 0 || output != *b"hi\n") as i32
}

fn random(state: &mut u64) -> i32 {
    let mut value = (*state % 0x7fff_fffe) + 1;
    let high = value / 127_773;
    let low = value % 127_773;
    value = 16_807 * low;
    let subtract = 2_836 * high;
    value = if value >= subtract {
        value - subtract
    } else {
        value + 0x7fff_ffff - subtract
    };
    value -= 1;
    *state = value;
    value as i32
}
