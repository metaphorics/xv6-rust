use alloc::alloc::{Layout, alloc, dealloc};
use core::cell::UnsafeCell;
use core::fmt::{self, Write as _};
use core::mem::align_of;
use core::sync::atomic::{AtomicBool, Ordering};

use ustd::abi::fcntl::{O_CREATE, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
use ustd::abi::{BSIZE, Dirent, MAXFILE, MAXOPBLOCKS, Sys};
use ustd::{
    chdir, close, dup, exec, exit, fork, getpid, kill, link, mkdir, open, pause, read, sbrk,
    unlink, wait, write,
};

const BUFSZ: usize = (MAXOPBLOCKS + 2) * BSIZE;
const NINODE: usize = 50;

macro_rules! fail {
    ($($arg:tt)*) => {{
        ustd::println!($($arg)*);
        exit(1)
    }};
}

fn name(name: &[u8]) -> &str {
    crate::display_name(name)
}

fn text(bytes: &[u8]) -> &str {
    let len = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..len]).unwrap_or("?")
}

struct FdWriter(i32);

impl fmt::Write for FdWriter {
    fn write_str(&mut self, string: &str) -> fmt::Result {
        if write(self.0, string.as_bytes()) == string.len() as isize {
            Ok(())
        } else {
            Err(fmt::Error)
        }
    }
}

fn fd_println(fd: i32, args: fmt::Arguments<'_>) {
    let mut writer = FdWriter(fd);
    let _ = writer.write_fmt(args);
    let _ = write(fd, b"\n");
}

fn raw_write(fd: i32, address: usize, len: usize) -> isize {
    // SAFETY: badwrite intentionally passes an invalid user address through the
    // raw syscall ABI without constructing a Rust reference to that address.
    unsafe { ustd::raw_syscall(Sys::Write, [fd as usize, address, len, 0, 0, 0]) }
}

struct GlobalBuffer {
    locked: AtomicBool,
    data: UnsafeCell<[u8; BUFSZ]>,
}

// SAFETY: `with` holds `locked` for the complete lifetime of the only reference
// it creates to `data`.
unsafe impl Sync for GlobalBuffer {}

impl GlobalBuffer {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new([0; BUFSZ]),
        }
    }

    fn with<R>(&'static self, use_buffer: impl FnOnce(&mut [u8; BUFSZ]) -> R) -> R {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let lock = BufferLock(self);
        // SAFETY: `lock` excludes every other call to this sole data accessor.
        let result = use_buffer(unsafe { &mut *self.data.get() });
        drop(lock);
        result
    }
}

struct BufferLock(&'static GlobalBuffer);

impl Drop for BufferLock {
    fn drop(&mut self) {
        self.0.locked.store(false, Ordering::Release);
    }
}

#[unsafe(link_section = ".bss.usertests_am")]
static BUF: GlobalBuffer = GlobalBuffer::new();

pub fn iput(test_name: &[u8]) {
    if mkdir(b"iputdir") < 0 {
        fail!("{}: mkdir failed", name(test_name));
    }
    if chdir(b"iputdir") < 0 {
        fail!("{}: chdir iputdir failed", name(test_name));
    }
    if unlink(b"../iputdir") < 0 {
        fail!("{}: unlink ../iputdir failed", name(test_name));
    }
    if chdir(b"/") < 0 {
        fail!("{}: chdir / failed", name(test_name));
    }
}

pub fn exitiput(test_name: &[u8]) {
    let pid = fork();
    if pid < 0 {
        fail!("{}: fork failed", name(test_name));
    }
    if pid == 0 {
        if mkdir(b"iputdir") < 0 {
            fail!("{}: mkdir failed", name(test_name));
        }
        if chdir(b"iputdir") < 0 {
            fail!("{}: child chdir failed", name(test_name));
        }
        if unlink(b"../iputdir") < 0 {
            fail!("{}: unlink ../iputdir failed", name(test_name));
        }
        exit(0);
    }
    let mut status = 0;
    wait(Some(&mut status));
    exit(status);
}

pub fn createtest(_: &[u8]) {
    const N: usize = 52;
    let mut file = [b'a', 0, 0];
    for i in 0..N {
        file[1] = b'0' + i as u8;
        let fd = open(&file, O_CREATE | O_RDWR);
        close(fd);
    }
    for i in 0..N {
        file[1] = b'0' + i as u8;
        unlink(&file);
    }
}

pub fn dirtest(test_name: &[u8]) {
    if mkdir(b"dir0") < 0 {
        fail!("{}: mkdir failed", name(test_name));
    }
    if chdir(b"dir0") < 0 {
        fail!("{}: chdir dir0 failed", name(test_name));
    }
    if chdir(b"..") < 0 {
        fail!("{}: chdir .. failed", name(test_name));
    }
    if unlink(b"dir0") < 0 {
        fail!("{}: unlink dir0 failed", name(test_name));
    }
}

pub fn exectest(test_name: &[u8]) {
    unlink(b"echo-ok");
    let pid = fork();
    if pid < 0 {
        fail!("{}: fork failed", name(test_name));
    }
    if pid == 0 {
        let errfd = dup(1);
        if errfd < 0 {
            fail!("{}: dup failed", name(test_name));
        }
        close(1);
        let fd = open(b"echo-ok", O_CREATE | O_WRONLY);
        if fd < 0 {
            fd_println(errfd, format_args!("{}: create failed", name(test_name)));
            exit(1);
        }
        if fd != 1 {
            fd_println(errfd, format_args!("{}: wrong fd", name(test_name)));
            exit(1);
        }
        if exec(b"echo", &[b"echo", b"OK"]) < 0 {
            fd_println(errfd, format_args!("{}: exec echo failed", name(test_name)));
            exit(1);
        }
    }

    let mut status = 0;
    if wait(Some(&mut status)) != pid {
        ustd::println!("{}: wait failed!", name(test_name));
    }
    if status != 0 {
        fail!("{}: nonzero wait status {}", name(test_name), status);
    }
    let fd = open(b"echo-ok", O_RDONLY);
    if fd < 0 {
        fail!("{}: open failed", name(test_name));
    }
    let mut output = [0; 3];
    if read(fd, &mut output[..2]) != 2 {
        fail!("{}: read failed", name(test_name));
    }
    unlink(b"echo-ok");
    if output[0] == b'O' && output[1] == b'K' {
        exit(0);
    }
    fail!("{}: wrong output", name(test_name));
}

pub fn killstatus(test_name: &[u8]) {
    for _ in 0..100 {
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test_name));
        }
        if pid == 0 {
            loop {
                getpid();
            }
        }
        pause(1);
        kill(pid);
        let mut status = 0;
        wait(Some(&mut status));
        if status != -1 {
            fail!("{}: status should be -1", name(test_name));
        }
    }
    exit(0);
}

pub fn exitwait(test_name: &[u8]) {
    for i in 0..100 {
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test_name));
        }
        if pid == 0 {
            exit(i);
        }
        let mut status = 0;
        if wait(Some(&mut status)) != pid {
            fail!("{}: wait wrong pid", name(test_name));
        }
        if status != i {
            fail!("{}: wait wrong exit status", name(test_name));
        }
    }
}

pub fn forkfork(test_name: &[u8]) {
    const N: usize = 2;
    for _ in 0..N {
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test_name));
        }
        if pid == 0 {
            for _ in 0..200 {
                let child = fork();
                if child < 0 {
                    exit(1);
                }
                if child == 0 {
                    exit(0);
                }
                wait(None);
            }
            exit(0);
        }
    }
    for _ in 0..N {
        let mut status = 0;
        wait(Some(&mut status));
        if status != 0 {
            fail!("{}: fork in child failed", name(test_name));
        }
    }
}

pub fn forkforkfork(test_name: &[u8]) {
    unlink(b"stopforking");
    let pid = fork();
    if pid < 0 {
        fail!("{}: fork failed", name(test_name));
    }
    if pid == 0 {
        loop {
            let fd = open(b"stopforking", 0);
            if fd >= 0 {
                exit(0);
            }
            if fork() < 0 {
                close(open(b"stopforking", O_CREATE | O_RDWR));
            }
        }
    }
    pause(20);
    close(open(b"stopforking", O_CREATE | O_RDWR));
    wait(None);
    pause(10);
}

pub fn mem(test_name: &[u8]) {
    let pid = fork();
    if pid == 0 {
        let Ok(small) = Layout::from_size_align(10_001, align_of::<usize>()) else {
            fail!("{}: couldn't allocate mem?!!", name(test_name));
        };
        let mut first = core::ptr::null_mut::<u8>();
        loop {
            // SAFETY: `small` is non-zero and uses an alignment accepted by the
            // process allocator. A null result is handled as allocation failure.
            let next = unsafe { alloc(small) };
            if next.is_null() {
                break;
            }
            // SAFETY: the allocation has room and alignment for one pointer.
            unsafe { next.cast::<*mut u8>().write(first) };
            first = next;
        }
        while !first.is_null() {
            // SAFETY: every list node is a live `small` allocation whose first
            // word was initialized to the preceding node.
            let next = unsafe { first.cast::<*mut u8>().read() };
            // SAFETY: `first` was returned by `alloc(small)` and is freed once.
            unsafe { dealloc(first, small) };
            first = next;
        }

        let Ok(large) = Layout::from_size_align(1024 * 20, align_of::<usize>()) else {
            fail!("{}: couldn't allocate mem?!!", name(test_name));
        };
        // SAFETY: `large` is a valid non-zero layout.
        let allocation = unsafe { alloc(large) };
        if allocation.is_null() {
            fail!("{}: couldn't allocate mem?!!", name(test_name));
        }
        // SAFETY: `allocation` was returned by `alloc(large)` and is freed once.
        unsafe { dealloc(allocation, large) };
        exit(0);
    }

    let mut status = 0;
    wait(Some(&mut status));
    if status == -1 {
        exit(0);
    }
    exit(status);
}

pub fn fourfiles(test_name: &[u8]) {
    const N: usize = 12;
    const NCHILD: usize = 4;
    const SZ: usize = 500;
    let files: [&[u8]; NCHILD] = [b"f0", b"f1", b"f2", b"f3"];

    for (pi, file) in files.iter().enumerate() {
        unlink(file);
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test_name));
        }
        if pid == 0 {
            let fd = open(file, O_CREATE | O_RDWR);
            if fd < 0 {
                fail!("{}: create failed", name(test_name));
            }
            BUF.with(|buf| {
                buf[..SZ].fill(b'0' + pi as u8);
                for _ in 0..N {
                    let written = write(fd, &buf[..SZ]);
                    if written != SZ as isize {
                        fail!("write failed {}", written);
                    }
                }
            });
            exit(0);
        }
    }

    for _ in 0..NCHILD {
        let mut status = 0;
        wait(Some(&mut status));
        if status != 0 {
            exit(status);
        }
    }

    for (i, file) in files.iter().enumerate() {
        let fd = open(file, 0);
        let total = BUF.with(|buf| {
            let mut total = 0;
            loop {
                let count = read(fd, buf);
                if count <= 0 {
                    break;
                }
                for byte in &buf[..count as usize] {
                    if *byte != b'0' + i as u8 {
                        fail!("{}: wrong char", name(test_name));
                    }
                }
                total += count;
            }
            total
        });
        close(fd);
        if total != (N * SZ) as isize {
            fail!("wrong length {}", total);
        }
        unlink(file);
    }
}

pub fn createdelete(test_name: &[u8]) {
    const N: usize = 20;
    const NCHILD: usize = 4;

    for pi in 0..NCHILD {
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test_name));
        }
        if pid == 0 {
            let mut file = [b'p' + pi as u8, 0, 0];
            for i in 0..N {
                file[1] = b'0' + i as u8;
                let fd = open(&file, O_CREATE | O_RDWR);
                if fd < 0 {
                    fail!("{}: create failed", name(test_name));
                }
                close(fd);
                if i > 0 && i % 2 == 0 {
                    file[1] = b'0' + (i / 2) as u8;
                    if unlink(&file) < 0 {
                        fail!("{}: unlink failed", name(test_name));
                    }
                }
            }
            exit(0);
        }
    }

    for _ in 0..NCHILD {
        let mut status = 0;
        wait(Some(&mut status));
        if status != 0 {
            exit(1);
        }
    }

    let mut file = [0; 3];
    for i in 0..N {
        for pi in 0..NCHILD {
            file[0] = b'p' + pi as u8;
            file[1] = b'0' + i as u8;
            let fd = open(&file, 0);
            if (i == 0 || i >= N / 2) && fd < 0 {
                fail!(
                    "{}: oops createdelete {} didn't exist",
                    name(test_name),
                    text(&file)
                );
            } else if (1..N / 2).contains(&i) && fd >= 0 {
                fail!(
                    "{}: oops createdelete {} did exist",
                    name(test_name),
                    text(&file)
                );
            }
            if fd >= 0 {
                close(fd);
            }
        }
    }

    for i in 0..N {
        for pi in 0..NCHILD {
            file[0] = b'p' + pi as u8;
            file[1] = b'0' + i as u8;
            unlink(&file);
        }
    }
}

pub fn linktest(test_name: &[u8]) {
    const SZ: usize = 5;
    unlink(b"lf1");
    unlink(b"lf2");

    let fd = open(b"lf1", O_CREATE | O_RDWR);
    if fd < 0 {
        fail!("{}: create lf1 failed", name(test_name));
    }
    if write(fd, b"hello") != SZ as isize {
        fail!("{}: write lf1 failed", name(test_name));
    }
    close(fd);

    if link(b"lf1", b"lf2") < 0 {
        fail!("{}: link lf1 lf2 failed", name(test_name));
    }
    unlink(b"lf1");
    if open(b"lf1", 0) >= 0 {
        fail!("{}: unlinked lf1 but it is still there!", name(test_name));
    }

    let fd = open(b"lf2", 0);
    if fd < 0 {
        fail!("{}: open lf2 failed", name(test_name));
    }
    let count = BUF.with(|buf| read(fd, buf));
    if count != SZ as isize {
        fail!("{}: read lf2 failed", name(test_name));
    }
    close(fd);

    if link(b"lf2", b"lf2") >= 0 {
        fail!("{}: link lf2 lf2 succeeded! oops", name(test_name));
    }
    unlink(b"lf2");
    if link(b"lf2", b"lf1") >= 0 {
        fail!("{}: link non-existent succeeded! oops", name(test_name));
    }
    if link(b".", b"lf1") >= 0 {
        fail!("{}: link . lf1 succeeded! oops", name(test_name));
    }
}

pub fn concreate(test_name: &[u8]) {
    const N: usize = 40;
    let mut file = [b'C', 0, 0];

    for i in 0..N {
        file[1] = b'0' + i as u8;
        unlink(&file);
        let pid = fork();
        if (pid != 0 && i % 3 == 1) || (pid == 0 && i % 5 == 1) {
            link(b"C0", &file);
        } else {
            let fd = open(&file, O_CREATE | O_RDWR);
            if fd < 0 {
                fail!("concreate create {} failed", text(&file));
            }
            close(fd);
        }
        if pid == 0 {
            exit(0);
        }
        let mut status = 0;
        wait(Some(&mut status));
        if status != 0 {
            exit(1);
        }
    }

    let mut found = [0_u8; N];
    let fd = open(b".", 0);
    let mut count = 0;
    let mut encoded = [0; Dirent::ENCODED_LEN];
    while read(fd, &mut encoded) > 0 {
        let Some(entry) = Dirent::decode(&encoded) else {
            continue;
        };
        if entry.inum == 0 {
            continue;
        }
        if entry.name[0] == b'C' && entry.name[2] == 0 {
            let index = i32::from(entry.name[1]) - i32::from(b'0');
            if index < 0 || index >= N as i32 {
                fail!(
                    "{}: concreate weird file {}",
                    name(test_name),
                    text(&entry.name)
                );
            }
            if found[index as usize] != 0 {
                fail!(
                    "{}: concreate duplicate file {}",
                    name(test_name),
                    text(&entry.name)
                );
            }
            found[index as usize] = 1;
            count += 1;
        }
    }
    close(fd);
    if count != N {
        fail!(
            "{}: concreate not enough files in directory listing",
            name(test_name)
        );
    }

    for i in 0..N {
        file[1] = b'0' + i as u8;
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test_name));
        }
        if (i % 3 == 0 && pid == 0) || (i % 3 == 1 && pid != 0) {
            for _ in 0..6 {
                close(open(&file, 0));
            }
        } else {
            for _ in 0..6 {
                unlink(&file);
            }
        }
        if pid == 0 {
            exit(0);
        }
        wait(None);
    }
}

pub fn linkunlink(test_name: &[u8]) {
    unlink(b"x");
    let pid = fork();
    if pid < 0 {
        fail!("{}: fork failed", name(test_name));
    }

    let mut random = if pid != 0 { 1_u32 } else { 97_u32 };
    for _ in 0..100 {
        random = random.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        match random % 3 {
            0 => {
                close(open(b"x", O_RDWR | O_CREATE));
            }
            1 => {
                link(b"cat", b"x");
            }
            _ => {
                unlink(b"x");
            }
        }
    }
    if pid != 0 {
        wait(None);
    } else {
        exit(0);
    }
}

pub fn bigwrite(test_name: &[u8]) {
    unlink(b"bigwrite");
    let mut size = 499;
    while size < BUFSZ {
        let fd = open(b"bigwrite", O_CREATE | O_RDWR);
        if fd < 0 {
            fail!("{}: cannot create bigwrite", name(test_name));
        }
        BUF.with(|buf| {
            for _ in 0..2 {
                let count = write(fd, &buf[..size]);
                if count != size as isize {
                    fail!("{}: write({}) ret {}", name(test_name), size, count);
                }
            }
        });
        close(fd);
        unlink(b"bigwrite");
        size += 471;
    }
}

pub fn bigfile(test_name: &[u8]) {
    const N: usize = 20;
    const SZ: usize = 600;
    unlink(b"bigfile.dat");
    let fd = open(b"bigfile.dat", O_CREATE | O_RDWR);
    if fd < 0 {
        fail!("{}: cannot create bigfile", name(test_name));
    }
    BUF.with(|buf| {
        for i in 0..N {
            buf[..SZ].fill(i as u8);
            if write(fd, &buf[..SZ]) != SZ as isize {
                fail!("{}: write bigfile failed", name(test_name));
            }
        }
    });
    close(fd);

    let fd = open(b"bigfile.dat", 0);
    if fd < 0 {
        fail!("{}: cannot open bigfile", name(test_name));
    }
    let total = BUF.with(|buf| {
        let mut total = 0;
        for i in 0.. {
            let count = read(fd, &mut buf[..SZ / 2]);
            if count < 0 {
                fail!("{}: read bigfile failed", name(test_name));
            }
            if count == 0 {
                break;
            }
            if count != (SZ / 2) as isize {
                fail!("{}: short read bigfile", name(test_name));
            }
            if buf[0] != (i / 2) as u8 || buf[SZ / 2 - 1] != (i / 2) as u8 {
                fail!("{}: read bigfile wrong data", name(test_name));
            }
            total += count;
        }
        total
    });
    close(fd);
    if total != (N * SZ) as isize {
        fail!("{}: read bigfile wrong total", name(test_name));
    }
    unlink(b"bigfile.dat");
}

pub fn fourteen(test_name: &[u8]) {
    if mkdir(b"12345678901234") != 0 {
        fail!("{}: mkdir 12345678901234 failed", name(test_name));
    }
    if mkdir(b"12345678901234/123456789012345") != 0 {
        fail!(
            "{}: mkdir 12345678901234/123456789012345 failed",
            name(test_name)
        );
    }
    let fd = open(b"123456789012345/123456789012345/123456789012345", O_CREATE);
    if fd < 0 {
        fail!(
            "{}: create 123456789012345/123456789012345/123456789012345 failed",
            name(test_name)
        );
    }
    close(fd);
    let fd = open(b"12345678901234/12345678901234/12345678901234", 0);
    if fd < 0 {
        fail!(
            "{}: open 12345678901234/12345678901234/12345678901234 failed",
            name(test_name)
        );
    }
    close(fd);
    if mkdir(b"12345678901234/12345678901234") == 0 {
        fail!(
            "{}: mkdir 12345678901234/12345678901234 succeeded!",
            name(test_name)
        );
    }
    if mkdir(b"123456789012345/12345678901234") == 0 {
        fail!(
            "{}: mkdir 12345678901234/123456789012345 succeeded!",
            name(test_name)
        );
    }

    unlink(b"123456789012345/12345678901234");
    unlink(b"12345678901234/12345678901234");
    unlink(b"12345678901234/12345678901234/12345678901234");
    unlink(b"123456789012345/123456789012345/123456789012345");
    unlink(b"12345678901234/123456789012345");
    unlink(b"12345678901234");
}

pub fn dirfile(test_name: &[u8]) {
    let fd = open(b"dirfile", O_CREATE);
    if fd < 0 {
        fail!("{}: create dirfile failed", name(test_name));
    }
    close(fd);
    if chdir(b"dirfile") == 0 {
        fail!("{}: chdir dirfile succeeded!", name(test_name));
    }
    if open(b"dirfile/xx", 0) >= 0 {
        fail!("{}: create dirfile/xx succeeded!", name(test_name));
    }
    if open(b"dirfile/xx", O_CREATE) >= 0 {
        fail!("{}: create dirfile/xx succeeded!", name(test_name));
    }
    if mkdir(b"dirfile/xx") == 0 {
        fail!("{}: mkdir dirfile/xx succeeded!", name(test_name));
    }
    if unlink(b"dirfile/xx") == 0 {
        fail!("{}: unlink dirfile/xx succeeded!", name(test_name));
    }
    if link(b"README", b"dirfile/xx") == 0 {
        fail!("{}: link to dirfile/xx succeeded!", name(test_name));
    }
    if unlink(b"dirfile") != 0 {
        fail!("{}: unlink dirfile failed!", name(test_name));
    }

    if open(b".", O_RDWR) >= 0 {
        fail!("{}: open . for writing succeeded!", name(test_name));
    }
    let fd = open(b".", 0);
    if write(fd, b"x") > 0 {
        fail!("{}: write . succeeded!", name(test_name));
    }
    close(fd);
}

pub fn iref(test_name: &[u8]) {
    for _ in 0..NINODE + 1 {
        if mkdir(b"irefd") != 0 {
            fail!("{}: mkdir irefd failed", name(test_name));
        }
        if chdir(b"irefd") != 0 {
            fail!("{}: chdir irefd failed", name(test_name));
        }
        mkdir(b"");
        link(b"README", b"");
        let fd = open(b"", O_CREATE);
        if fd >= 0 {
            close(fd);
        }
        let fd = open(b"xx", O_CREATE);
        if fd >= 0 {
            close(fd);
        }
        unlink(b"xx");
    }
    for _ in 0..NINODE + 1 {
        chdir(b"..");
        unlink(b"irefd");
    }
    chdir(b"/");
}

pub fn forktest(test_name: &[u8]) {
    const N: usize = 1000;
    let mut count = 0;
    while count < N {
        let pid = fork();
        if pid < 0 {
            break;
        }
        if pid == 0 {
            exit(0);
        }
        count += 1;
    }
    if count == 0 {
        fail!("{}: no fork at all!", name(test_name));
    }
    if count == N {
        fail!("{}: fork claimed to work 1000 times!", name(test_name));
    }
    while count > 0 {
        if wait(None) < 0 {
            fail!("{}: wait stopped early", name(test_name));
        }
        count -= 1;
    }
    if wait(None) != -1 {
        fail!("{}: wait got too many", name(test_name));
    }
}

pub fn bigdir(test_name: &[u8]) {
    const N: usize = 500;
    unlink(b"bd");
    let fd = open(b"bd", O_CREATE);
    if fd < 0 {
        fail!("{}: bigdir create failed", name(test_name));
    }
    close(fd);

    let mut file = [b'x', 0, 0, 0];
    for i in 0..N {
        file[1] = b'0' + (i / 64) as u8;
        file[2] = b'0' + (i % 64) as u8;
        if link(b"bd", &file) != 0 {
            fail!(
                "{}: bigdir i={} link(bd, {}) failed",
                name(test_name),
                i,
                text(&file)
            );
        }
    }
    unlink(b"bd");
    for i in 0..N {
        file[1] = b'0' + (i / 64) as u8;
        file[2] = b'0' + (i % 64) as u8;
        if unlink(&file) != 0 {
            fail!("{}: bigdir unlink failed", name(test_name));
        }
    }
}

pub fn manywrites(test_name: &[u8]) {
    let nchildren = 4;
    let howmany = 30;
    for child_index in 0..nchildren {
        let pid = fork();
        if pid < 0 {
            fail!("fork failed");
        }
        if pid == 0 {
            let file = [b'b', b'a' + child_index as u8, 0];
            unlink(&file);
            for _ in 0..howmany {
                for _ in 0..child_index + 1 {
                    let fd = open(&file, O_CREATE | O_RDWR);
                    if fd < 0 {
                        fail!("{}: cannot create {}", name(test_name), text(&file));
                    }
                    let count = BUF.with(|buf| write(fd, buf));
                    if count != BUFSZ as isize {
                        fail!("{}: write({}) ret {}", name(test_name), BUFSZ, count);
                    }
                    close(fd);
                }
                unlink(&file);
            }
            unlink(&file);
            exit(0);
        }
    }
    for _ in 0..nchildren {
        let mut status = 0;
        wait(Some(&mut status));
        if status != 0 {
            exit(status);
        }
    }
    exit(0);
}

pub fn badwrite(_: &[u8]) {
    let assumed_free = 600;
    unlink(b"junk");
    for _ in 0..assumed_free {
        let fd = open(b"junk", O_CREATE | O_WRONLY);
        if fd < 0 {
            fail!("open junk failed");
        }
        let _ = raw_write(fd, 0xffffffffff, 1);
        close(fd);
        unlink(b"junk");
    }

    let fd = open(b"junk", O_CREATE | O_WRONLY);
    if fd < 0 {
        fail!("open junk failed");
    }
    if write(fd, b"x") != 1 {
        fail!("write failed");
    }
    close(fd);
    unlink(b"junk");
    exit(0);
}

pub fn execout(_: &[u8]) {
    const PAGE_SIZE: isize = 4096;
    for available in 0..15 {
        let pid = fork();
        if pid < 0 {
            fail!("fork failed");
        }
        if pid == 0 {
            loop {
                let address = sbrk(PAGE_SIZE);
                if address == -1 {
                    break;
                }
                // SAFETY: successful sbrk returned a fresh writable page; the
                // last byte lies within that allocation.
                unsafe { ((address + PAGE_SIZE - 1) as *mut u8).write_volatile(1) };
            }
            for _ in 0..available {
                sbrk(-PAGE_SIZE);
            }
            close(1);
            let _ = exec(b"echo", &[b"echo", b"x"]);
            exit(0);
        }
        wait(None);
    }
    exit(0);
}

pub fn diskfull(test_name: &[u8]) {
    let mut file_index = 0_u8;
    let mut done = false;
    unlink(b"diskfulldir");

    while !done && b'0' + file_index < 0o177 {
        let file = [b'b', b'i', b'g', b'0' + file_index, 0];
        unlink(&file);
        let fd = open(&file, O_CREATE | O_RDWR | O_TRUNC);
        if fd < 0 {
            ustd::println!("{}: could not create file {}", name(test_name), text(&file));

            break;
        }
        for _ in 0..MAXFILE {
            let block = [0; BSIZE];
            if write(fd, &block) != BSIZE as isize {
                done = true;
                close(fd);
                break;
            }
        }
        close(fd);
        file_index += 1;
    }

    let nzz = 128;
    for i in 0..nzz {
        let file = [b'z', b'z', b'0' + (i / 32) as u8, b'0' + (i % 32) as u8, 0];
        unlink(&file);
        let fd = open(&file, O_CREATE | O_RDWR | O_TRUNC);
        if fd < 0 {
            break;
        }
        close(fd);
    }

    if mkdir(b"diskfulldir") == 0 {
        ustd::println!(
            "{}: mkdir(diskfulldir) unexpectedly succeeded!",
            name(test_name)
        );
    }
    unlink(b"diskfulldir");

    for i in 0..nzz {
        let file = [b'z', b'z', b'0' + (i / 32) as u8, b'0' + (i % 32) as u8, 0];
        unlink(&file);
    }
    for i in 0_u8.. {
        if b'0' + i >= 0o177 {
            break;
        }
        let file = [b'b', b'i', b'g', b'0' + i, 0];
        unlink(&file);
    }
}
