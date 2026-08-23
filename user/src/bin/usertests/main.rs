#![no_std]
#![no_main]

extern crate alloc;

mod tests_am;
mod tests_core;
mod tests_nz;

use ustd::{entry, exit, fork, print, println, sbrk, wait};

const PAGE_SIZE: isize = 4096;

type TestFn = fn(&[u8]);

#[derive(Clone, Copy)]
struct Test {
    run: TestFn,
    name: &'static [u8],
}

const QUICK_TESTS: [Test; 67] = [
    Test {
        run: tests_core::copyin,
        name: b"copyin",
    },
    Test {
        run: tests_core::copyout,
        name: b"copyout",
    },
    Test {
        run: tests_core::copyinstr1,
        name: b"copyinstr1",
    },
    Test {
        run: tests_core::copyinstr2,
        name: b"copyinstr2",
    },
    Test {
        run: tests_core::copyinstr3,
        name: b"copyinstr3",
    },
    Test {
        run: tests_core::rwsbrk,
        name: b"rwsbrk",
    },
    Test {
        run: tests_nz::truncate1,
        name: b"truncate1",
    },
    Test {
        run: tests_nz::truncate2,
        name: b"truncate2",
    },
    Test {
        run: tests_nz::truncate3,
        name: b"truncate3",
    },
    Test {
        run: tests_nz::openiput,
        name: b"openiput",
    },
    Test {
        run: tests_am::exitiput,
        name: b"exitiput",
    },
    Test {
        run: tests_am::iput,
        name: b"iput",
    },
    Test {
        run: tests_nz::opentest,
        name: b"opentest",
    },
    Test {
        run: tests_nz::writetest,
        name: b"writetest",
    },
    Test {
        run: tests_nz::writebig,
        name: b"writebig",
    },
    Test {
        run: tests_am::createtest,
        name: b"createtest",
    },
    Test {
        run: tests_am::dirtest,
        name: b"dirtest",
    },
    Test {
        run: tests_am::exectest,
        name: b"exectest",
    },
    Test {
        run: tests_nz::pipe1,
        name: b"pipe1",
    },
    Test {
        run: tests_am::killstatus,
        name: b"killstatus",
    },
    Test {
        run: tests_nz::preempt,
        name: b"preempt",
    },
    Test {
        run: tests_am::exitwait,
        name: b"exitwait",
    },
    Test {
        run: tests_nz::reparent,
        name: b"reparent",
    },
    Test {
        run: tests_nz::twochildren,
        name: b"twochildren",
    },
    Test {
        run: tests_am::forkfork,
        name: b"forkfork",
    },
    Test {
        run: tests_am::forkforkfork,
        name: b"forkforkfork",
    },
    Test {
        run: tests_nz::reparent2,
        name: b"reparent2",
    },
    Test {
        run: tests_am::mem,
        name: b"mem",
    },
    Test {
        run: tests_nz::sharedfd,
        name: b"sharedfd",
    },
    Test {
        run: tests_am::fourfiles,
        name: b"fourfiles",
    },
    Test {
        run: tests_am::createdelete,
        name: b"createdelete",
    },
    Test {
        run: tests_nz::unlinkread,
        name: b"unlinkread",
    },
    Test {
        run: tests_am::linktest,
        name: b"linktest",
    },
    Test {
        run: tests_am::concreate,
        name: b"concreate",
    },
    Test {
        run: tests_am::linkunlink,
        name: b"linkunlink",
    },
    Test {
        run: tests_nz::subdir,
        name: b"subdir",
    },
    Test {
        run: tests_am::bigwrite,
        name: b"bigwrite",
    },
    Test {
        run: tests_am::bigfile,
        name: b"bigfile",
    },
    Test {
        run: tests_am::fourteen,
        name: b"fourteen",
    },
    Test {
        run: tests_nz::rmdot,
        name: b"rmdot",
    },
    Test {
        run: tests_am::dirfile,
        name: b"dirfile",
    },
    Test {
        run: tests_am::iref,
        name: b"iref",
    },
    Test {
        run: tests_am::forktest,
        name: b"forktest",
    },
    Test {
        run: tests_core::sbrkbasic,
        name: b"sbrkbasic",
    },
    Test {
        run: tests_core::sbrkmuch,
        name: b"sbrkmuch",
    },
    Test {
        run: tests_core::kernmem,
        name: b"kernmem",
    },
    Test {
        run: tests_core::maxva_plus,
        name: b"MAXVAplus",
    },
    Test {
        run: tests_core::sbrkfail,
        name: b"sbrkfail",
    },
    Test {
        run: tests_core::sbrkarg,
        name: b"sbrkarg",
    },
    Test {
        run: tests_core::validatetest,
        name: b"validatetest",
    },
    Test {
        run: tests_core::bsstest,
        name: b"bsstest",
    },
    Test {
        run: tests_core::bigargtest,
        name: b"bigargtest",
    },
    Test {
        run: tests_core::argptest,
        name: b"argptest",
    },
    Test {
        run: tests_core::stacktest,
        name: b"stacktest",
    },
    Test {
        run: tests_core::nowrite,
        name: b"nowrite",
    },
    Test {
        run: tests_core::pgbug,
        name: b"pgbug",
    },
    Test {
        run: tests_core::sbrkbugs,
        name: b"sbrkbugs",
    },
    Test {
        run: tests_core::sbrklast,
        name: b"sbrklast",
    },
    Test {
        run: tests_core::sbrk8000,
        name: b"sbrk8000",
    },
    Test {
        run: tests_core::badarg,
        name: b"badarg",
    },
    Test {
        run: tests_core::lazy_alloc,
        name: b"lazy_alloc",
    },
    Test {
        run: tests_core::lazy_unmap,
        name: b"lazy_unmap",
    },
    Test {
        run: tests_core::lazy_copy,
        name: b"lazy_copy",
    },
    Test {
        run: tests_core::lazy_copyinstr,
        name: b"lazy_copyinstr",
    },
    Test {
        run: tests_core::lazy_sbrk,
        name: b"lazy_sbrk",
    },
    Test {
        run: tests_core::partial_write,
        name: b"partial_write",
    },
    Test {
        run: tests_nz::unlinkcwd,
        name: b"unlinkcwd",
    },
];

const SLOW_TESTS: [Test; 6] = [
    Test {
        run: tests_am::bigdir,
        name: b"bigdir",
    },
    Test {
        run: tests_am::manywrites,
        name: b"manywrites",
    },
    Test {
        run: tests_am::badwrite,
        name: b"badwrite",
    },
    Test {
        run: tests_am::execout,
        name: b"execout",
    },
    Test {
        run: tests_am::diskfull,
        name: b"diskfull",
    },
    Test {
        run: tests_nz::outofinodes,
        name: b"outofinodes",
    },
];

fn display_name(name: &[u8]) -> &str {
    core::str::from_utf8(name).unwrap_or("?")
}

fn run(test: Test) -> bool {
    print!("test {}: ", display_name(test.name));
    let pid = fork();
    if pid < 0 {
        println!("runtest: fork error");
        exit(1);
    }
    if pid == 0 {
        (test.run)(test.name);
        exit(0);
    }
    let mut status = 0;
    let _ = wait(Some(&mut status));
    if status == 0 {
        println!("OK");
    } else {
        println!("FAILED");
    }
    status == 0
}

fn run_tests(tests: &[Test], just_one: Option<&[u8]>, continuous: u8) -> isize {
    let mut count = 0;
    for &test in tests {
        if just_one.is_none_or(|name| name == test.name) {
            count += 1;
            if !run(test) && continuous != 2 {
                println!("SOME TESTS FAILED");
                return -1;
            }
        }
    }
    count
}

fn countfree() -> usize {
    let original = sbrk(0);
    let mut pages = 0;
    while sbrk(PAGE_SIZE) != -1 {
        pages += 1;
    }
    let current = sbrk(0);
    let _ = sbrk(-(current - original));
    pages
}

fn drive(quick: bool, continuous: u8, just_one: Option<&[u8]>) -> i32 {
    loop {
        println!("usertests starting");
        let free_before = countfree();
        let mut count = run_tests(&QUICK_TESTS, just_one, continuous);
        if count < 0 && continuous != 2 {
            return 1;
        }
        if !quick {
            if just_one.is_none() {
                println!("usertests slow tests starting");
            }
            let slow_count = run_tests(&SLOW_TESTS, just_one, continuous);
            if slow_count < 0 && continuous != 2 {
                return 1;
            }
            if slow_count >= 0 {
                count += slow_count;
            }
        }
        let free_after = countfree();
        if free_after < free_before {
            println!(
                "FAILED -- lost some free pages {} (out of {})",
                free_after, free_before
            );
            if continuous != 2 {
                return 1;
            }
        }
        if just_one.is_some() && count == 0 {
            println!("NO TESTS EXECUTED");
            return 1;
        }
        if continuous == 0 {
            return 0;
        }
    }
}

fn main(args: &[&[u8]]) -> i32 {
    let (quick, continuous, just_one) = match args {
        [_program] => (false, 0, None),
        [_program, flag] if *flag == b"-q" => (true, 0, None),
        [_program, flag] if *flag == b"-c" => (false, 1, None),
        [_program, flag] if *flag == b"-C" => (false, 2, None),
        [_program, test] if !test.starts_with(b"-") => (false, 0, Some(*test)),
        _ => {
            println!("Usage: usertests [-c] [-C] [-q] [testname]");
            return 1;
        }
    };
    if drive(quick, continuous, just_one) != 0 {
        return 1;
    }
    println!("ALL TESTS PASSED");
    0
}

entry!(main);
