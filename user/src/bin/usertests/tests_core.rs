use alloc::vec;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use ustd::abi::fcntl::{O_CREATE, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
use ustd::abi::{FileType, Sys};
use ustd::{
    close, exit, fork, fstat, kill, open, pause, pipe, read, sbrk, sbrklazy, unlink, wait, write,
};

const PAGE_SIZE: usize = 4096;
const MAXPATH: usize = 128;
const MAXARG: usize = 32;
const USERSTACK: usize = 1;
const KERNBASE: usize = 0x8000_0000;
const MAXVA: usize = 1 << 38;
const TRAPFRAME: usize = MAXVA - 2 * PAGE_SIZE;
const REGION_SIZE: isize = 1024 * 1024 * 1024;
const BUFSZ: usize = (10 + 2) * 1024;

macro_rules! fail {
    ($($arg:tt)*) => {{
        ustd::println!($($arg)*);
        exit(1)
    }};
}

fn name(name: &[u8]) -> &str {
    crate::display_name(name)
}

fn raw(number: Sys, a0: usize, a1: usize, a2: usize) -> isize {
    // SAFETY: each test intentionally controls the raw syscall ABI. Invalid
    // pointers are passed as integers so Rust never constructs invalid refs.
    unsafe { ustd::raw_syscall(number, [a0, a1, a2, 0, 0, 0]) }
}

fn raw_read(fd: i32, address: usize, len: usize) -> isize {
    raw(Sys::Read, fd as usize, address, len)
}

fn raw_write(fd: i32, address: usize, len: usize) -> isize {
    raw(Sys::Write, fd as usize, address, len)
}

fn write_byte(address: usize, value: u8) {
    // SAFETY: callers use an address returned by sbrk, except in forked
    // fault tests where the deliberate invalid access must kill the child.
    unsafe { (address as *mut u8).write_volatile(value) };
}

fn read_byte(address: usize) -> u8 {
    // SAFETY: same contract as `write_byte`.
    unsafe { (address as *const u8).read_volatile() }
}

fn write_word(address: usize, value: usize) {
    // SAFETY: callers use naturally aligned page addresses returned by sbrk.
    unsafe { (address as *mut usize).write_volatile(value) };
}

fn read_word(address: usize) -> usize {
    // SAFETY: callers use naturally aligned page addresses returned by sbrk.
    unsafe { (address as *const usize).read_volatile() }
}

struct GlobalBuffer {
    locked: AtomicBool,
    data: UnsafeCell<[u8; BUFSZ]>,
}

// SAFETY: `lock` serializes every access to `data`.
unsafe impl Sync for GlobalBuffer {}

impl GlobalBuffer {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new([0; BUFSZ]),
        }
    }

    fn lock(&'static self) -> BufferGuard {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        BufferGuard { buffer: self }
    }
}

struct BufferGuard {
    buffer: &'static GlobalBuffer,
}

impl Deref for BufferGuard {
    type Target = [u8; BUFSZ];

    fn deref(&self) -> &Self::Target {
        // SAFETY: this guard owns the lock until drop.
        unsafe { &*self.buffer.data.get() }
    }
}

impl DerefMut for BufferGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: this guard owns the lock and is the only mutable accessor.
        unsafe { &mut *self.buffer.data.get() }
    }
}

impl Drop for BufferGuard {
    fn drop(&mut self) {
        self.buffer.locked.store(false, Ordering::Release);
    }
}

static BUF: GlobalBuffer = GlobalBuffer::new();
static UNINIT: [u8; 10_000] = [0; 10_000];

pub fn copyin(_: &[u8]) {
    let addrs = [
        0x8000_0000,
        0x3fff_ffe000,
        0x3fff_fff000,
        0x4000_000000,
        usize::MAX,
    ];
    for addr in addrs {
        let fd = open(b"copyin1", O_CREATE | O_WRONLY);
        if fd < 0 {
            fail!("open(copyin1) failed");
        }
        let n = raw_write(fd, addr, 8192);
        if n >= 0 {
            fail!("write(fd, {:#x}, 8192) returned {}, not -1", addr, n);
        }
        close(fd);
        unlink(b"copyin1");

        let n = raw_write(1, addr, 8192);
        if n > 0 {
            fail!("write(1, {:#x}, 8192) returned {}, not -1 or 0", addr, n);
        }

        let mut fds = [0; 2];
        if pipe(&mut fds) < 0 {
            fail!("pipe() failed");
        }
        let n = raw_write(fds[1], addr, 8192);
        if n > 0 {
            fail!("write(pipe, {:#x}, 8192) returned {}, not -1 or 0", addr, n);
        }
        close(fds[0]);
        close(fds[1]);
    }
}

pub fn copyout(_: &[u8]) {
    let addrs = [
        0,
        0x8000_0000,
        0x3fff_ffe000,
        0x3fff_fff000,
        0x4000_000000,
        usize::MAX,
    ];
    for addr in addrs {
        let fd = open(b"README", O_RDONLY);
        if fd < 0 {
            fail!("open(README) failed");
        }
        let n = raw_read(fd, addr, 8192);
        if n > 0 {
            fail!("read(fd, {:#x}, 8192) returned {}, not -1 or 0", addr, n);
        }
        close(fd);

        let mut fds = [0; 2];
        if pipe(&mut fds) < 0 {
            fail!("pipe() failed");
        }
        if write(fds[1], b"x") != 1 {
            fail!("pipe write failed");
        }
        let n = raw_read(fds[0], addr, 8192);
        if n > 0 {
            fail!("read(pipe, {:#x}, 8192) returned {}, not -1 or 0", addr, n);
        }
        close(fds[0]);
        close(fds[1]);
    }
}

pub fn copyinstr1(_: &[u8]) {
    let addrs = [
        0x8000_0000,
        0x3fff_ffe000,
        0x3fff_fff000,
        0x4000_000000,
        usize::MAX,
    ];
    for addr in addrs {
        let fd = raw(Sys::Open, addr, (O_CREATE | O_WRONLY) as usize, 0);
        if fd >= 0 {
            fail!("open({:#x}) returned {}, not -1", addr, fd);
        }
    }
}

pub fn copyinstr2(_: &[u8]) {
    let mut path = [b'x'; MAXPATH + 1];
    path[MAXPATH] = 0;
    let address = path.as_ptr() as usize;
    if raw(Sys::Unlink, address, 0, 0) != -1 {
        fail!("unlink(long path) did not return -1");
    }
    if raw(Sys::Open, address, (O_CREATE | O_WRONLY) as usize, 0) != -1 {
        fail!("open(long path) did not return -1");
    }
    if raw(Sys::Link, address, address, 0) != -1 {
        fail!("link(long path, long path) did not return -1");
    }
    let argv = [c"xx".as_ptr() as usize, 0];
    if raw(Sys::Exec, address, argv.as_ptr() as usize, 0) != -1 {
        fail!("exec(long path) did not return -1");
    }

    let pid = fork();
    if pid < 0 {
        fail!("fork failed");
    }
    if pid == 0 {
        let mut big = vec![b'x'; PAGE_SIZE + 1];
        big[PAGE_SIZE] = 0;
        let argv = [
            big.as_ptr() as usize,
            big.as_ptr() as usize,
            big.as_ptr() as usize,
            0,
        ];
        let ret = raw(
            Sys::Exec,
            c"echo".as_ptr() as usize,
            argv.as_ptr() as usize,
            0,
        );
        if ret != -1 {
            fail!("exec(echo, BIG) returned {}, not -1", ret);
        }
        exit(747);
    }
    let mut status = 0;
    wait(Some(&mut status));
    if status != 747 {
        fail!("exec(echo, BIG) succeeded, should have failed");
    }
}

pub fn copyinstr3(_: &[u8]) {
    let _ = sbrk(8192);
    let mut top = sbrk(0) as usize;
    if !top.is_multiple_of(PAGE_SIZE) {
        let _ = sbrk((PAGE_SIZE - top % PAGE_SIZE) as isize);
    }
    top = sbrk(0) as usize;
    if !top.is_multiple_of(PAGE_SIZE) {
        fail!("oops");
    }
    let address = top - 1;
    write_byte(address, b'x');
    if raw(Sys::Unlink, address, 0, 0) != -1 {
        fail!("unlink(cross-page path) did not return -1");
    }
    if raw(Sys::Open, address, (O_CREATE | O_WRONLY) as usize, 0) != -1 {
        fail!("open(cross-page path) did not return -1");
    }
    if raw(Sys::Link, address, address, 0) != -1 {
        fail!("link(cross-page path) did not return -1");
    }
    let argv = [c"xx".as_ptr() as usize, 0];
    if raw(Sys::Exec, address, argv.as_ptr() as usize, 0) != -1 {
        fail!("exec(cross-page path) did not return -1");
    }
}

pub fn rwsbrk(_: &[u8]) {
    let address = sbrk(8192);
    if address == -1 {
        fail!("sbrk(rwsbrk) failed");
    }
    if sbrk(-8192) == -1 {
        fail!("sbrk(rwsbrk) shrink failed");
    }
    let invalid = address as usize + PAGE_SIZE;
    let fd = open(b"rwsbrk", O_CREATE | O_WRONLY);
    if fd < 0 {
        fail!("open(rwsbrk) failed");
    }
    let n = raw_write(fd, invalid, 1024);
    if n >= 0 {
        fail!("write(fd, {:#x}, 1024) returned {}, not -1", invalid, n);
    }
    close(fd);
    unlink(b"rwsbrk");

    let fd = open(b"README", O_RDONLY);
    if fd < 0 {
        fail!("open(README) failed");
    }
    let n = raw_read(fd, invalid, 10);
    if n >= 0 {
        fail!("read(fd, {:#x}, 10) returned {}, not -1", invalid, n);
    }
    close(fd);
}

pub fn sbrkbasic(test: &[u8]) {
    const TOO_MUCH: isize = 1024 * 1024 * 1024;
    let pid = fork();
    if pid < 0 {
        fail!("fork failed in sbrkbasic");
    }
    if pid == 0 {
        let start = sbrk(TOO_MUCH);
        if start == -1 {
            exit(0);
        }
        let mut address = start as usize;
        let end = address + TOO_MUCH as usize;
        while address < end {
            write_byte(address, 99);
            address += PAGE_SIZE;
        }
        exit(1);
    }
    let mut status = 0;
    wait(Some(&mut status));
    if status == 1 {
        fail!("{}: too much memory allocated!", name(test));
    }

    let mut expected = sbrk(0);
    for index in 0..5000 {
        let actual = sbrk(1);
        if actual != expected {
            fail!(
                "{}: sbrk test failed {} {:#x} {:#x}",
                name(test),
                index,
                expected,
                actual
            );
        }
        write_byte(actual as usize, 1);
        expected = actual + 1;
    }
    let pid = fork();
    if pid < 0 {
        fail!("{}: sbrk test fork failed", name(test));
    }
    let _ = sbrk(1);
    let actual = sbrk(1);
    if actual != expected + 1 {
        fail!("{}: sbrk test failed post-fork", name(test));
    }
    if pid == 0 {
        exit(0);
    }
    wait(Some(&mut status));
    exit(status);
}

pub fn sbrkmuch(test: &[u8]) {
    const BIG: isize = 100 * 1024 * 1024;
    let old_break = sbrk(0);
    let start = sbrk(0);
    let amount = BIG - start;
    let actual = sbrk(amount);
    if actual != start {
        fail!(
            "{}: sbrk test failed to grow big address space; enough phys mem?",
            name(test)
        );
    }
    let last = BIG as usize - 1;
    write_byte(last, 99);

    let before = sbrk(0);
    if sbrk(-(PAGE_SIZE as isize)) == -1 {
        fail!("{}: sbrk could not deallocate", name(test));
    }
    let after = sbrk(0);
    if after != before - PAGE_SIZE as isize {
        fail!(
            "{}: sbrk deallocation produced wrong address, a {:#x} c {:#x}",
            name(test),
            before,
            after
        );
    }
    let before = sbrk(0);
    let actual = sbrk(PAGE_SIZE as isize);
    if actual != before || sbrk(0) != before + PAGE_SIZE as isize {
        fail!(
            "{}: sbrk re-allocation failed, a {:#x} c {:#x}",
            name(test),
            before,
            actual
        );
    }
    if read_byte(last) == 99 {
        fail!(
            "{}: sbrk de-allocation didn't really deallocate",
            name(test)
        );
    }
    let before = sbrk(0);
    let actual = sbrk(-(sbrk(0) - old_break));
    if actual != before {
        fail!(
            "{}: sbrk downsize failed, a {:#x} c {:#x}",
            name(test),
            before,
            actual
        );
    }
}

pub fn kernmem(test: &[u8]) {
    let mut address = KERNBASE;
    while address < KERNBASE + 2_000_000 {
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test));
        }
        if pid == 0 {
            let value = read_byte(address);
            fail!(
                "{}: oops could read {:#x} = {:#x}",
                name(test),
                address,
                value
            );
        }
        let mut status = 0;
        wait(Some(&mut status));
        if status != -1 {
            exit(1);
        }
        address += 50_000;
    }
}

pub fn maxva_plus(test: &[u8]) {
    let mut address = MAXVA;
    while address != 0 {
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test));
        }
        if pid == 0 {
            write_byte(address, 99);
            fail!("{}: oops wrote {:#x}", name(test), address);
        }
        let mut status = 0;
        wait(Some(&mut status));
        if status != -1 {
            exit(1);
        }
        address <<= 1;
    }
}

pub fn sbrkfail(test: &[u8]) {
    const BIG: isize = 100 * 1024 * 1024;
    let mut fds = [0; 2];
    if pipe(&mut fds) != 0 {
        fail!("{}: pipe() failed", name(test));
    }
    let mut pids = [-1; 10];
    let mut failed = false;
    for slot in &mut pids {
        *slot = fork();
        if *slot == 0 {
            let result = sbrk(BIG - sbrk(0));
            let marker = if result == -1 { b"0" } else { b"1" };
            let _ = write(fds[1], marker);
            loop {
                pause(1000);
            }
        }
        if *slot != -1 {
            let mut marker = [0];
            let _ = read(fds[0], &mut marker);
            failed |= marker[0] == b'0';
        }
    }
    if !failed {
        ustd::println!("{}: no allocation failed; allocate more?", name(test));
    }
    let page = sbrk(PAGE_SIZE as isize);
    for pid in pids {
        if pid != -1 {
            kill(pid);
            wait(None);
        }
    }
    if page == -1 {
        fail!("{}: failed sbrk leaked memory", name(test));
    }

    let pid = fork();
    if pid < 0 {
        fail!("{}: fork failed", name(test));
    }
    if pid == 0 {
        if sbrk(10 * BIG) == -1 {
            exit(0);
        }
        fail!(
            "{}: allocate a lot of memory succeeded {}",
            name(test),
            10 * BIG
        );
    }
    let mut status = 0;
    wait(Some(&mut status));
    if status != 0 {
        exit(1);
    }
}

pub fn sbrkarg(test: &[u8]) {
    let address = sbrk(PAGE_SIZE as isize);
    let fd = open(b"sbrk", O_CREATE | O_WRONLY);
    unlink(b"sbrk");
    if fd < 0 {
        fail!("{}: open sbrk failed", name(test));
    }
    if raw_write(fd, address as usize, PAGE_SIZE) < 0 {
        fail!("{}: write sbrk failed", name(test));
    }
    close(fd);

    let address = sbrk(PAGE_SIZE as isize);
    if raw(Sys::Pipe, address as usize, 0, 0) != 0 {
        fail!("{}: pipe() failed", name(test));
    }
}

pub fn validatetest(test: &[u8]) {
    let old = b"nosuchfile\0";
    for address in (0..=1100 * 1024).step_by(PAGE_SIZE) {
        if raw(Sys::Link, old.as_ptr() as usize, address, 0) != -1 {
            fail!("{}: link should not succeed", name(test));
        }
    }
}

pub fn bsstest(test: &[u8]) {
    if UNINIT.iter().any(|&byte| byte != 0) {
        fail!("{}: bss test failed", name(test));
    }
}

pub fn bigargtest(test: &[u8]) {
    unlink(b"bigarg-ok");
    let pid = fork();
    if pid == 0 {
        let mut big = [b' '; 400];
        big[399] = 0;
        let mut argv = [0usize; MAXARG];
        argv[..MAXARG - 1].fill(big.as_ptr() as usize);
        let _ = raw(
            Sys::Exec,
            c"echo".as_ptr() as usize,
            argv.as_ptr() as usize,
            0,
        );
        let fd = open(b"bigarg-ok", O_CREATE);
        close(fd);
        exit(0);
    }
    if pid < 0 {
        fail!("{}: bigargtest: fork failed", name(test));
    }
    let mut status = 0;
    wait(Some(&mut status));
    if status != 0 {
        exit(status);
    }
    let fd = open(b"bigarg-ok", O_RDONLY);
    if fd < 0 {
        fail!("{}: bigarg test failed!", name(test));
    }
    close(fd);
}

pub fn argptest(_: &[u8]) {
    let fd = open(b"init", O_RDONLY);
    if fd < 0 {
        fail!("argptest: open failed");
    }
    let _ = raw_read(fd, sbrk(0) as usize - 1, usize::MAX);
    close(fd);
}

fn stack_pointer() -> usize {
    let sp: usize;
    #[cfg(target_arch = "riscv64")]
    // SAFETY: reading the current stack pointer has no side effects.
    unsafe {
        core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags))
    };
    #[cfg(target_arch = "x86_64")]
    // SAFETY: reading the current stack pointer has no side effects.
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags))
    };
    sp
}

pub fn stacktest(test: &[u8]) {
    let pid = fork();
    if pid == 0 {
        let address = stack_pointer() - USERSTACK * PAGE_SIZE;
        let value = read_byte(address);
        fail!("{}: stacktest: read below stack {}", name(test), value);
    }
    if pid < 0 {
        fail!("{}: fork failed", name(test));
    }
    let mut status = 0;
    wait(Some(&mut status));
    if status == -1 {
        exit(0);
    }
    exit(status);
}

pub fn nowrite(test: &[u8]) {
    let addrs = [
        0,
        0x8000_0000,
        0x3fff_ffe000,
        0x3fff_fff000,
        0x4000_000000,
        usize::MAX,
    ];
    for address in addrs {
        let pid = fork();
        if pid == 0 {
            write_word(address, 10);
            ustd::println!("{}: write to {:#x} did not fail!", name(test), address);
            exit(0);
        }
        if pid < 0 {
            fail!("{}: fork failed", name(test));
        }
        let mut status = 0;
        wait(Some(&mut status));
        if status == 0 {
            exit(1);
        }
    }
    exit(0);
}

pub fn pgbug(_: &[u8]) {
    const BIG: usize = 0xeaeb_0b5b_0000_2f5e;
    let argv = [0usize];
    let _ = raw(Sys::Exec, BIG, argv.as_ptr() as usize, 0);
    let _ = raw(Sys::Pipe, BIG, 0, 0);
    exit(0);
}

pub fn sbrkbugs(_: &[u8]) {
    let pid = fork();
    if pid < 0 {
        fail!("fork failed");
    }
    if pid == 0 {
        let size = sbrk(0);
        let _ = sbrk(-size);
        exit(0);
    }
    wait(None);

    let pid = fork();
    if pid < 0 {
        fail!("fork failed");
    }
    if pid == 0 {
        let size = sbrk(0);
        let _ = sbrk(-(size - 3500));
        exit(0);
    }
    wait(None);

    let pid = fork();
    if pid < 0 {
        fail!("fork failed");
    }
    if pid == 0 {
        let _ = sbrk((10 * PAGE_SIZE + 2048) as isize - sbrk(0));
        let _ = sbrk(-10);
        exit(0);
    }
    wait(None);
    exit(0);
}

pub fn sbrklast(_: &[u8]) {
    let mut top = sbrk(0) as usize;
    if !top.is_multiple_of(PAGE_SIZE) {
        let _ = sbrk((PAGE_SIZE - top % PAGE_SIZE) as isize);
    }
    let _ = sbrk(PAGE_SIZE as isize);
    let _ = sbrk(10);
    let _ = sbrk(-20);
    top = sbrk(0) as usize;
    let path = top - 64;
    write_byte(path, b'x');
    write_byte(path + 1, 0);
    let fd = raw(Sys::Open, path, (O_RDWR | O_CREATE) as usize, 0) as i32;
    let _ = raw_write(fd, path, 1);
    close(fd);
    let fd = raw(Sys::Open, path, O_RDWR as usize, 0) as i32;
    write_byte(path, 0);
    let _ = raw_read(fd, path, 1);
    if read_byte(path) != b'x' {
        exit(1);
    }
    close(fd);
}

pub fn sbrk8000(_: &[u8]) {
    let _ = sbrk(0x8000_0004u32 as i32 as isize);
    let top = sbrk(0) as usize;
    let value = read_byte(top - 1);
    write_byte(top - 1, value.wrapping_add(1));
}

pub fn badarg(_: &[u8]) {
    let argv = [0xffff_ffffusize, 0];
    for _ in 0..50_000 {
        let _ = raw(
            Sys::Exec,
            c"echo".as_ptr() as usize,
            argv.as_ptr() as usize,
            0,
        );
    }
    exit(0);
}

pub fn lazy_alloc(_: &[u8]) {
    let previous = sbrklazy(REGION_SIZE);
    if previous == -1 {
        fail!("sbrklazy() failed");
    }
    let end = previous as usize + REGION_SIZE as usize;
    let mut address = previous as usize + PAGE_SIZE;
    while address < end {
        write_word(address, address);
        address += 64 * PAGE_SIZE;
    }
    address = previous as usize + PAGE_SIZE;
    while address < end {
        if read_word(address) != address {
            fail!("failed to read value from memory");
        }
        address += 64 * PAGE_SIZE;
    }
    exit(0);
}

pub fn lazy_unmap(_: &[u8]) {
    let previous = sbrklazy(REGION_SIZE);
    if previous == -1 {
        fail!("sbrklazy() failed");
    }
    let end = previous as usize + REGION_SIZE as usize;
    let mut address = previous as usize + PAGE_SIZE;
    while address < end {
        write_word(address, address);
        address += PAGE_SIZE * PAGE_SIZE;
    }
    address = previous as usize + PAGE_SIZE;
    while address < end {
        let pid = fork();
        if pid < 0 {
            fail!("error forking");
        }
        if pid == 0 {
            let _ = sbrklazy(-REGION_SIZE);
            write_word(address, address);
            exit(0);
        }
        let mut status = 0;
        wait(Some(&mut status));
        if status == 0 {
            fail!("memory not unmapped");
        }
        address += PAGE_SIZE * PAGE_SIZE;
    }
    exit(0);
}

pub fn lazy_copy(_: &[u8]) {
    let path = sbrk(0) as usize;
    let _ = sbrklazy((4 * PAGE_SIZE) as isize);
    let _ = raw(Sys::Open, path + 8192, 0, 0);

    let current = sbrk(0);
    let returned = sbrk(-(current + 1));
    if returned != current {
        fail!("sbrk(sbrk(0)+1) returned {:#x}, not old sz", returned);
    }

    let bad = [
        0x3fff_ffc000,
        0x3fff_ffd000,
        0x3fff_ffe000,
        0x3fff_fff000,
        0x4000_000000,
        0x8000_000000,
    ];
    for address in bad {
        let fd = open(b"README", O_RDONLY);
        if fd < 0 {
            fail!("cannot open README");
        }
        if raw_read(fd, address, 512) >= 0 {
            fail!("read succeeded");
        }
        close(fd);
        let fd = open(b"junk", O_CREATE | O_RDWR | O_TRUNC);
        if fd < 0 {
            fail!("cannot open junk");
        }
        if raw_write(fd, address, 512) >= 0 {
            fail!("write succeeded");
        }
        close(fd);
    }
    exit(0);
}

pub fn lazy_copyinstr(test: &[u8]) {
    let p = sbrk(0) as usize;
    let _ = sbrk((PAGE_SIZE - p % PAGE_SIZE) as isize);
    let p = sbrk(0) as usize;
    if !p.is_multiple_of(PAGE_SIZE) {
        fail!("{}: sbrk did not align", name(test));
    }
    let _ = sbrklazy((2 * PAGE_SIZE) as isize);
    write_byte(p + PAGE_SIZE - 1, b'/');
    let fd = raw(Sys::Open, p + PAGE_SIZE - 1, O_RDONLY as usize, 0) as i32;
    if fd < 0 {
        fail!("could not open /");
    }
    let stat = fstat(fd).unwrap_or_else(|_| fail!("could not stat /"));
    if stat.r#type != FileType::Dir as i16 {
        fail!("/ is not T_DIR");
    }
    close(fd);
}

pub fn lazy_sbrk(_: &[u8]) {
    let mut current = sbrk(0);
    while (current as usize) < MAXVA - (1 << 30) {
        let previous = sbrklazy(1 << 30);
        if previous == -1 {
            fail!("sbrklazy({}) returned {:#x}", 1 << 30, previous);
        }
        current = sbrklazy(0);
    }
    let amount = (TRAPFRAME - PAGE_SIZE - current as usize) as isize;
    let previous = sbrklazy(amount);
    if previous == -1 || previous != current {
        fail!(
            "sbrklazy({}) returned {:#x}, not expected {:#x}",
            amount,
            previous,
            current
        );
    }
    let page = sbrk(PAGE_SIZE as isize);
    if page == -1 || page as usize != TRAPFRAME - PAGE_SIZE {
        fail!(
            "sbrk({}) returned {:#x}, not expected TRAPFRAME-PGSIZE",
            PAGE_SIZE,
            page
        );
    }
    write_byte(page as usize, 1);
    if read_byte(page as usize + 1) != 0 {
        fail!("sbrk() returned non-zero-filled memory");
    }
    if sbrk(1) != -1 {
        fail!("sbrk(1) did not return error");
    }
    if sbrklazy(1) != -1 {
        fail!("sbrklazy(1) did not return error");
    }
    exit(0);
}

pub fn partial_write(test: &[u8]) {
    unlink(b"testfile");
    let mut fd = open(b"testfile", O_CREATE | O_RDWR);
    if fd < 0 {
        fail!("{}: cannot create testfile", name(test));
    }
    if write(fd, b"A") != 1 {
        fail!("{}: could not write A", name(test));
    }
    close(fd);
    fd = open(b"testfile", O_RDWR);
    if fd < 0 {
        fail!("{}: cannot re-open testfile", name(test));
    }

    let p = sbrk(0) as usize;
    let _ = sbrk((PAGE_SIZE - p % PAGE_SIZE) as isize);
    let p = sbrk(0) as usize;
    if !p.is_multiple_of(PAGE_SIZE) {
        fail!("{}: sbrk did not align", name(test));
    }
    write_byte(p - 1, b'X');
    if raw_write(fd, p - 1, 2) != -1 {
        fail!("{}: write succeeded, should have failed", name(test));
    }
    close(fd);

    let mut byte = [0];
    fd = open(b"testfile", O_RDONLY);
    if fd < 0 || read(fd, &mut byte) != 1 {
        fail!("{}: cannot read testfile", name(test));
    }
    close(fd);
    if byte[0] != b'X' {
        fail!(
            "{}: read returned {}, expected X",
            name(test),
            byte[0] as char
        );
    }

    fd = open(b"bigfile", O_CREATE | O_RDWR);
    let mut buffer = BUF.lock();
    buffer[..1024].fill(0);
    for _ in 0..64 {
        if write(fd, &buffer[..1024]) != 1024 {
            fail!("{}: could not write to bigfile", name(test));
        }
    }
    drop(buffer);
    close(fd);
    unlink(b"bigfile");

    fd = open(b"testfile", O_RDONLY);
    if fd < 0 || read(fd, &mut byte) != 1 {
        fail!("{}: cannot read testfile", name(test));
    }
    close(fd);
    if byte[0] != b'X' {
        fail!(
            "{}: read returned {}, expected X",
            name(test),
            byte[0] as char
        );
    }
    unlink(b"testfile");
}
