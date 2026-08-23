use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use ustd::abi::fcntl::{O_CREATE, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
use ustd::abi::{BSIZE, MAXFILE, MAXOPBLOCKS};
use ustd::{
    chdir, close, exit, fork, getpid, kill, link, mkdir, open, pause, pipe, read, unlink, wait,
    write,
};

const BUFSZ: usize = (MAXOPBLOCKS + 2) * BSIZE;

macro_rules! fail {
    ($($arg:tt)*) => {{
        ustd::println!($($arg)*);
        exit(1)
    }};
}

fn name(name: &[u8]) -> &str {
    crate::display_name(name)
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

pub fn truncate1(test: &[u8]) {
    let mut buffer = [0; 32];

    unlink(b"truncfile");
    let mut fd1 = open(b"truncfile", O_CREATE | O_WRONLY | O_TRUNC);
    write(fd1, b"abcd");
    close(fd1);

    let fd2 = open(b"truncfile", O_RDONLY);
    let mut n = read(fd2, &mut buffer);
    if n != 4 {
        fail!("{}: read {} bytes, wanted 4", name(test), n);
    }

    fd1 = open(b"truncfile", O_WRONLY | O_TRUNC);

    let fd3 = open(b"truncfile", O_RDONLY);
    n = read(fd3, &mut buffer);
    if n != 0 {
        ustd::println!("aaa fd3={}", fd3);
        fail!("{}: read {} bytes, wanted 0", name(test), n);
    }

    n = read(fd2, &mut buffer);
    if n != 0 {
        ustd::println!("bbb fd2={}", fd2);
        fail!("{}: read {} bytes, wanted 0", name(test), n);
    }

    write(fd1, b"abcdef");

    n = read(fd3, &mut buffer);
    if n != 6 {
        fail!("{}: read {} bytes, wanted 6", name(test), n);
    }

    n = read(fd2, &mut buffer);
    if n != 2 {
        fail!("{}: read {} bytes, wanted 2", name(test), n);
    }

    unlink(b"truncfile");
    close(fd1);
    close(fd2);
    close(fd3);
}

pub fn truncate2(test: &[u8]) {
    unlink(b"truncfile");

    let fd1 = open(b"truncfile", O_CREATE | O_TRUNC | O_WRONLY);
    write(fd1, b"abcd");

    let fd2 = open(b"truncfile", O_TRUNC | O_WRONLY);

    let n = write(fd1, b"x");
    if n != -1 {
        fail!("{}: write returned {}, expected -1", name(test), n);
    }

    unlink(b"truncfile");
    close(fd1);
    close(fd2);
}

pub fn truncate3(test: &[u8]) {
    close(open(b"truncfile", O_CREATE | O_TRUNC | O_WRONLY));

    let pid = fork();
    if pid < 0 {
        fail!("{}: fork failed", name(test));
    }

    if pid == 0 {
        for _ in 0..100 {
            let mut buffer = [0; 32];
            let mut fd = open(b"truncfile", O_WRONLY);
            if fd < 0 {
                fail!("{}: open failed", name(test));
            }
            let n = write(fd, b"1234567890");
            if n != 10 {
                fail!("{}: write got {}, expected 10", name(test), n);
            }
            close(fd);
            fd = open(b"truncfile", O_RDONLY);
            read(fd, &mut buffer);
            close(fd);
        }
        exit(0);
    }

    for _ in 0..150 {
        let fd = open(b"truncfile", O_CREATE | O_WRONLY | O_TRUNC);
        if fd < 0 {
            fail!("{}: open failed", name(test));
        }
        let n = write(fd, b"xxx");
        if n != 3 {
            fail!("{}: write got {}, expected 3", name(test), n);
        }
        close(fd);
    }

    let mut status = 0;
    wait(Some(&mut status));
    unlink(b"truncfile");
    exit(status);
}

pub fn openiput(test: &[u8]) {
    if mkdir(b"oidir") < 0 {
        fail!("{}: mkdir oidir failed", name(test));
    }
    let pid = fork();
    if pid < 0 {
        fail!("{}: fork failed", name(test));
    }
    if pid == 0 {
        let fd = open(b"oidir", O_RDWR);
        if fd >= 0 {
            fail!("{}: open directory for write succeeded", name(test));
        }
        exit(0);
    }
    pause(1);
    if unlink(b"oidir") != 0 {
        fail!("{}: unlink failed", name(test));
    }
    let mut status = 0;
    wait(Some(&mut status));
    exit(status);
}

pub fn opentest(test: &[u8]) {
    let fd = open(b"echo", O_RDONLY);
    if fd < 0 {
        fail!("{}: open echo failed!", name(test));
    }
    close(fd);
    let fd = open(b"doesnotexist", O_RDONLY);
    if fd >= 0 {
        fail!("{}: open doesnotexist succeeded!", name(test));
    }
}

pub fn writetest(test: &[u8]) {
    const N: usize = 100;
    const SZ: usize = 10;

    let mut fd = open(b"small", O_CREATE | O_RDWR);
    if fd < 0 {
        fail!("{}: error: creat small failed!", name(test));
    }
    for i in 0..N {
        if write(fd, b"aaaaaaaaaa") != SZ as isize {
            fail!("{}: error: write aa {} new file failed", name(test), i);
        }
        if write(fd, b"bbbbbbbbbb") != SZ as isize {
            fail!("{}: error: write bb {} new file failed", name(test), i);
        }
    }
    close(fd);
    fd = open(b"small", O_RDONLY);
    if fd < 0 {
        fail!("{}: error: open small failed!", name(test));
    }
    let mut buffer = BUF.lock();
    let count = read(fd, &mut buffer[..N * SZ * 2]);
    if count != (N * SZ * 2) as isize {
        fail!("{}: read failed", name(test));
    }
    drop(buffer);
    close(fd);

    if unlink(b"small") < 0 {
        fail!("{}: unlink small failed", name(test));
    }
}

pub fn writebig(test: &[u8]) {
    let fd = open(b"big", O_CREATE | O_RDWR);
    if fd < 0 {
        fail!("{}: error: creat big failed!", name(test));
    }

    let mut buffer = BUF.lock();
    for i in 0..MAXFILE {
        buffer[..4].copy_from_slice(&(i as i32).to_ne_bytes());
        if write(fd, &buffer[..BSIZE]) != BSIZE as isize {
            fail!("{}: error: write big file failed i={}", name(test), i);
        }
    }

    close(fd);

    let fd = open(b"big", O_RDONLY);
    if fd < 0 {
        fail!("{}: error: open big failed!", name(test));
    }

    let mut block = 0;
    loop {
        let count = read(fd, &mut buffer[..BSIZE]);
        if count == 0 {
            if block != MAXFILE {
                ustd::print!("{}: read only {} blocks from big", name(test), block);
                exit(1);
            }
            break;
        } else if count != BSIZE as isize {
            fail!("{}: read failed {}", name(test), count);
        }
        let value = i32::from_ne_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        if value != block as i32 {
            fail!(
                "{}: read content of block {} is {}",
                name(test),
                block,
                value
            );
        }
        block += 1;
    }
    drop(buffer);
    close(fd);
    if unlink(b"big") < 0 {
        fail!("{}: unlink big failed", name(test));
    }
}

pub fn pipe1(test: &[u8]) {
    const N: usize = 5;
    const SZ: usize = 1033;

    let mut fds = [0; 2];
    if pipe(&mut fds) != 0 {
        fail!("{}: pipe() failed", name(test));
    }
    let pid = fork();
    let mut seq = 0usize;
    if pid == 0 {
        close(fds[0]);
        let mut buffer = BUF.lock();
        for _ in 0..N {
            for byte in &mut buffer[..SZ] {
                *byte = seq as u8;
                seq += 1;
            }
            if write(fds[1], &buffer[..SZ]) != SZ as isize {
                fail!("{}: pipe1 oops 1", name(test));
            }
        }
        exit(0);
    } else if pid > 0 {
        close(fds[1]);
        let mut total = 0usize;
        let mut count = 1usize;
        let mut buffer = BUF.lock();
        loop {
            let n = read(fds[0], &mut buffer[..count]);
            if n <= 0 {
                break;
            }
            for byte in &buffer[..n as usize] {
                if *byte != (seq & 0xff) as u8 {
                    ustd::println!("{}: pipe1 oops 2", name(test));
                    return;
                }
                seq += 1;
            }
            total += n as usize;
            count *= 2;
            if count > BUFSZ {
                count = BUFSZ;
            }
        }
        drop(buffer);
        if total != N * SZ {
            fail!("{}: pipe1 oops 3 total {}", name(test), total);
        }
        close(fds[0]);
        let mut status = 0;
        wait(Some(&mut status));
        exit(status);
    } else {
        fail!("{}: fork() failed", name(test));
    }
}

pub fn preempt(test: &[u8]) {
    let pid1 = fork();
    if pid1 < 0 {
        ustd::print!("{}: fork failed", name(test));
        exit(1);
    }
    if pid1 == 0 {
        loop {
            core::hint::spin_loop();
        }
    }

    let pid2 = fork();
    if pid2 < 0 {
        fail!("{}: fork failed", name(test));
    }
    if pid2 == 0 {
        loop {
            core::hint::spin_loop();
        }
    }

    let mut fds = [0; 2];
    pipe(&mut fds);
    let pid3 = fork();
    if pid3 < 0 {
        fail!("{}: fork failed", name(test));
    }
    if pid3 == 0 {
        close(fds[0]);
        if write(fds[1], b"x") != 1 {
            ustd::print!("{}: preempt write error", name(test));
        }
        close(fds[1]);
        loop {
            core::hint::spin_loop();
        }
    }

    close(fds[1]);
    let mut buffer = BUF.lock();
    if read(fds[0], &mut buffer[..]) != 1 {
        ustd::print!("{}: preempt read error", name(test));
        return;
    }
    drop(buffer);
    close(fds[0]);
    ustd::print!("kill... ");
    kill(pid1);
    kill(pid2);
    kill(pid3);
    ustd::print!("wait... ");
    wait(None);
    wait(None);
    wait(None);
}

pub fn reparent(test: &[u8]) {
    let master_pid = getpid();
    for _ in 0..200 {
        let pid = fork();
        if pid < 0 {
            fail!("{}: fork failed", name(test));
        }
        if pid != 0 {
            if wait(None) != pid {
                fail!("{}: wait wrong pid", name(test));
            }
        } else {
            let pid2 = fork();
            if pid2 < 0 {
                kill(master_pid);
                exit(1);
            }
            exit(0);
        }
    }
    exit(0);
}

pub fn twochildren(test: &[u8]) {
    for _ in 0..1000 {
        let pid1 = fork();
        if pid1 < 0 {
            fail!("{}: fork failed", name(test));
        }
        if pid1 == 0 {
            exit(0);
        } else {
            let pid2 = fork();
            if pid2 < 0 {
                fail!("{}: fork failed", name(test));
            }
            if pid2 == 0 {
                exit(0);
            } else {
                wait(None);
                wait(None);
            }
        }
    }
}

pub fn reparent2(_: &[u8]) {
    for _ in 0..800 {
        let pid1 = fork();
        if pid1 < 0 {
            fail!("fork failed");
        }
        if pid1 == 0 {
            fork();
            fork();
            exit(0);
        }
        wait(None);
    }
    exit(0);
}

pub fn sharedfd(test: &[u8]) {
    const N: usize = 1000;
    const SZ: usize = 10;

    unlink(b"sharedfd");
    let mut fd = open(b"sharedfd", O_CREATE | O_RDWR);
    if fd < 0 {
        ustd::print!("{}: cannot open sharedfd for writing", name(test));
        exit(1);
    }
    let pid = fork();
    let mut buffer = [if pid == 0 { b'c' } else { b'p' }; SZ];
    for _ in 0..N {
        if write(fd, &buffer) != SZ as isize {
            fail!("{}: write sharedfd failed", name(test));
        }
    }
    if pid == 0 {
        exit(0);
    } else {
        let mut status = 0;
        wait(Some(&mut status));
        if status != 0 {
            exit(status);
        }
    }

    close(fd);
    fd = open(b"sharedfd", O_RDONLY);
    if fd < 0 {
        fail!("{}: cannot open sharedfd for reading", name(test));
    }
    let mut nc = 0;
    let mut np = 0;
    loop {
        let count = read(fd, &mut buffer);
        if count <= 0 {
            break;
        }
        for byte in buffer {
            if byte == b'c' {
                nc += 1;
            }
            if byte == b'p' {
                np += 1;
            }
        }
    }
    close(fd);
    unlink(b"sharedfd");
    if nc == N * SZ && np == N * SZ {
        exit(0);
    } else {
        fail!("{}: nc/np test fails", name(test));
    }
}

pub fn unlinkread(test: &[u8]) {
    const SZ: isize = 5;

    let mut fd = open(b"unlinkread", O_CREATE | O_RDWR);
    if fd < 0 {
        fail!("{}: create unlinkread failed", name(test));
    }
    write(fd, b"hello");
    close(fd);

    fd = open(b"unlinkread", O_RDWR);
    if fd < 0 {
        fail!("{}: open unlinkread failed", name(test));
    }
    if unlink(b"unlinkread") != 0 {
        fail!("{}: unlink unlinkread failed", name(test));
    }

    let fd1 = open(b"unlinkread", O_CREATE | O_RDWR);
    write(fd1, b"yyy");
    close(fd1);

    let mut buffer = BUF.lock();
    if read(fd, &mut buffer[..]) != SZ {
        ustd::print!("{}: unlinkread read failed", name(test));
        exit(1);
    }
    if buffer[0] != b'h' {
        fail!("{}: unlinkread wrong data", name(test));
    }
    if write(fd, &buffer[..10]) != 10 {
        fail!("{}: unlinkread write failed", name(test));
    }
    drop(buffer);
    close(fd);
    unlink(b"unlinkread");
}

pub fn subdir(test: &[u8]) {
    unlink(b"ff");
    if mkdir(b"dd") != 0 {
        fail!("{}: mkdir dd failed", name(test));
    }

    let mut fd = open(b"dd/ff", O_CREATE | O_RDWR);
    if fd < 0 {
        fail!("{}: create dd/ff failed", name(test));
    }
    write(fd, b"ff");
    close(fd);

    if unlink(b"dd") >= 0 {
        fail!("{}: unlink dd (non-empty dir) succeeded!", name(test));
    }

    if mkdir(b"/dd/dd") != 0 {
        fail!("{}: subdir mkdir dd/dd failed", name(test));
    }

    fd = open(b"dd/dd/ff", O_CREATE | O_RDWR);
    if fd < 0 {
        fail!("{}: create dd/dd/ff failed", name(test));
    }
    write(fd, b"FF");
    close(fd);

    fd = open(b"dd/dd/../ff", O_RDONLY);
    if fd < 0 {
        fail!("{}: open dd/dd/../ff failed", name(test));
    }
    let mut buffer = BUF.lock();
    let count = read(fd, &mut buffer[..]);
    if count != 2 || buffer[0] != b'f' {
        fail!("{}: dd/dd/../ff wrong content", name(test));
    }
    close(fd);

    if link(b"dd/dd/ff", b"dd/dd/ffff") != 0 {
        fail!("{}: link dd/dd/ff dd/dd/ffff failed", name(test));
    }

    if unlink(b"dd/dd/ff") != 0 {
        fail!("{}: unlink dd/dd/ff failed", name(test));
    }
    if open(b"dd/dd/ff", O_RDONLY) >= 0 {
        fail!("{}: open (unlinked) dd/dd/ff succeeded", name(test));
    }

    if chdir(b"dd") != 0 {
        fail!("{}: chdir dd failed", name(test));
    }
    if chdir(b"dd/../../dd") != 0 {
        fail!("{}: chdir dd/../../dd failed", name(test));
    }
    if chdir(b"dd/../../../dd") != 0 {
        fail!("{}: chdir dd/../../../dd failed", name(test));
    }
    if chdir(b"./..") != 0 {
        fail!("{}: chdir ./.. failed", name(test));
    }

    fd = open(b"dd/dd/ffff", O_RDONLY);
    if fd < 0 {
        fail!("{}: open dd/dd/ffff failed", name(test));
    }
    if read(fd, &mut buffer[..]) != 2 {
        fail!("{}: read dd/dd/ffff wrong len", name(test));
    }
    close(fd);

    if open(b"dd/dd/ff", O_RDONLY) >= 0 {
        fail!("{}: open (unlinked) dd/dd/ff succeeded!", name(test));
    }

    if open(b"dd/ff/ff", O_CREATE | O_RDWR) >= 0 {
        fail!("{}: create dd/ff/ff succeeded!", name(test));
    }
    if open(b"dd/xx/ff", O_CREATE | O_RDWR) >= 0 {
        fail!("{}: create dd/xx/ff succeeded!", name(test));
    }
    if open(b"dd", O_CREATE) >= 0 {
        fail!("{}: create dd succeeded!", name(test));
    }
    if open(b"dd", O_RDWR) >= 0 {
        fail!("{}: open dd rdwr succeeded!", name(test));
    }
    if open(b"dd", O_WRONLY) >= 0 {
        fail!("{}: open dd wronly succeeded!", name(test));
    }
    if link(b"dd/ff/ff", b"dd/dd/xx") == 0 {
        fail!("{}: link dd/ff/ff dd/dd/xx succeeded!", name(test));
    }
    if link(b"dd/xx/ff", b"dd/dd/xx") == 0 {
        fail!("{}: link dd/xx/ff dd/dd/xx succeeded!", name(test));
    }
    if link(b"dd/ff", b"dd/dd/ffff") == 0 {
        fail!("{}: link dd/ff dd/dd/ffff succeeded!", name(test));
    }
    if mkdir(b"dd/ff/ff") == 0 {
        fail!("{}: mkdir dd/ff/ff succeeded!", name(test));
    }
    if mkdir(b"dd/xx/ff") == 0 {
        fail!("{}: mkdir dd/xx/ff succeeded!", name(test));
    }
    if mkdir(b"dd/dd/ffff") == 0 {
        fail!("{}: mkdir dd/dd/ffff succeeded!", name(test));
    }
    if unlink(b"dd/xx/ff") == 0 {
        fail!("{}: unlink dd/xx/ff succeeded!", name(test));
    }
    if unlink(b"dd/ff/ff") == 0 {
        fail!("{}: unlink dd/ff/ff succeeded!", name(test));
    }
    if chdir(b"dd/ff") == 0 {
        fail!("{}: chdir dd/ff succeeded!", name(test));
    }
    if chdir(b"dd/xx") == 0 {
        fail!("{}: chdir dd/xx succeeded!", name(test));
    }

    drop(buffer);
    if unlink(b"dd/dd/ffff") != 0 {
        fail!("{}: unlink dd/dd/ff failed", name(test));
    }
    if unlink(b"dd/ff") != 0 {
        fail!("{}: unlink dd/ff failed", name(test));
    }
    if unlink(b"dd") == 0 {
        fail!("{}: unlink non-empty dd succeeded!", name(test));
    }
    if unlink(b"dd/dd") < 0 {
        fail!("{}: unlink dd/dd failed", name(test));
    }
    if unlink(b"dd") < 0 {
        fail!("{}: unlink dd failed", name(test));
    }
}

pub fn rmdot(test: &[u8]) {
    if mkdir(b"dots") != 0 {
        fail!("{}: mkdir dots failed", name(test));
    }
    if chdir(b"dots") != 0 {
        fail!("{}: chdir dots failed", name(test));
    }
    if unlink(b".") == 0 {
        fail!("{}: rm . worked!", name(test));
    }
    if unlink(b"..") == 0 {
        fail!("{}: rm .. worked!", name(test));
    }
    if chdir(b"/") != 0 {
        fail!("{}: chdir / failed", name(test));
    }
    if unlink(b"dots/.") == 0 {
        fail!("{}: unlink dots/. worked!", name(test));
    }
    if unlink(b"dots/..") == 0 {
        fail!("{}: unlink dots/.. worked!", name(test));
    }
    if unlink(b"dots") != 0 {
        fail!("{}: unlink dots failed!", name(test));
    }
}

pub fn unlinkcwd(test: &[u8]) {
    if mkdir(b"/a") < 0 {
        fail!("{}: mkdir /a failed", name(test));
    }
    if mkdir(b"/a/b") < 0 {
        fail!("{}: mkdir /a/b failed", name(test));
    }
    if chdir(b"/a/b") < 0 {
        fail!("{}: chdir failed", name(test));
    }
    if unlink(b"/a/b") < 0 {
        fail!("{}: unlink /a/b failed", name(test));
    }
    if unlink(b"/a") < 0 {
        fail!("{}: unlink /a failed", name(test));
    }
    if open(b"../", O_RDONLY) > 0 {
        ustd::println!("{}: open ../ non-existing directory", name(test));
    }
    if open(b"../c", O_CREATE) > 0 {
        ustd::println!("{}: create ../c non-existing file", name(test));
    }
}

pub fn outofinodes(_: &[u8]) {
    const NZZ: usize = 32 * 32;

    for i in 0..NZZ {
        let mut file = [0; 32];
        file[0] = b'z';
        file[1] = b'z';
        file[2] = b'0' + (i / 32) as u8;
        file[3] = b'0' + (i % 32) as u8;
        unlink(&file[..4]);
        let fd = open(&file[..4], O_CREATE | O_RDWR | O_TRUNC);
        if fd < 0 {
            break;
        }
        close(fd);
    }

    for i in 0..NZZ {
        let mut file = [0; 32];
        file[0] = b'z';
        file[1] = b'z';
        file[2] = b'0' + (i / 32) as u8;
        file[3] = b'0' + (i % 32) as u8;
        unlink(&file[..4]);
    }
}
