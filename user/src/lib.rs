#![no_std]

//! Small xv6 user runtime: process entry, syscall wrappers, formatting,
//! input helpers, and the K&R `sbrk` allocator.

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::fmt::{self, Write as _};
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

pub use abi;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error;
pub use abi::Stat;

const MAXARG: usize = 32;
const MAXPATH: usize = 128;
const ARG_MAX_BYTES: usize = 4096;

#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
            // SAFETY: exec enters `_start` with argc and a nul-terminated
            // argv vector copied into this process's mapped user stack.
            unsafe { $crate::start_main(argc, argv, $main) }
        }
    };
}

/// Convert the raw exec ABI into safe byte slices once, call the program,
/// then terminate with its status.
///
/// # Safety
/// `argv` must point to `argc` readable pointers, each naming a readable
/// nul-terminated string of at most one page, as established by exec.
pub unsafe fn start_main(argc: usize, argv: *const *const u8, main: fn(&[&[u8]]) -> i32) -> ! {
    if argc > MAXARG {
        exit(-1);
    }
    let mut args: [&[u8]; MAXARG] = [&[]; MAXARG];
    for (index, slot) in args[..argc].iter_mut().enumerate() {
        // SAFETY: guaranteed by this function's ABI precondition.
        let string = unsafe { argv.add(index).read() };
        let mut len = 0;
        while len < ARG_MAX_BYTES {
            // SAFETY: exec validated and copied this string into the mapped
            // stack; the page-sized bound prevents an unbounded walk.
            if unsafe { string.add(len).read() } == 0 {
                break;
            }
            len += 1;
        }
        if len == ARG_MAX_BYTES {
            exit(-1);
        }
        // SAFETY: the scan above proved the first `len` bytes readable and
        // before the terminating nul. Each argv string occupies disjoint
        // immutable stack storage for the duration of `main`.
        *slot = unsafe { core::slice::from_raw_parts(string, len) };
    }
    exit(main(&args[..argc]));
}

#[cfg(target_arch = "riscv64")]
fn syscall(number: abi::Sys, args: [usize; 6]) -> isize {
    let mut a0 = args[0];
    // SAFETY: `ecall` is the RISC-V user ABI boundary. All pointer-bearing
    // wrappers below keep their referenced storage alive for the call.
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") a0,
            in("a1") args[1],
            in("a2") args[2],
            in("a3") args[3],
            in("a4") args[4],
            in("a5") args[5],
            in("a7") number as usize,
            options(nostack)
        );
    }
    a0 as isize
}

#[cfg(target_arch = "x86_64")]
fn syscall(number: abi::Sys, args: [usize; 6]) -> isize {
    let mut result = number as usize;
    // SAFETY: `int 0x80` is the x86_64 user ABI boundary installed by the
    // kernel. Pointer-bearing wrappers keep their storage live for the call.
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") result,
            in("rdi") args[0],
            in("rsi") args[1],
            in("rdx") args[2],
            in("r10") args[3],
            in("r8") args[4],
            in("r9") args[5],
            options(nostack)
        );
    }
    result as isize
}
/// Invoke an xv6 syscall with raw register arguments.
///
/// # Safety
/// Pointer-valued arguments must satisfy the selected syscall's ABI. This is
/// exposed for usertests that deliberately pass invalid addresses to verify
/// that the kernel rejects them without dereferencing them in supervisor mode.
pub unsafe fn raw_syscall(number: abi::Sys, args: [usize; 6]) -> isize {
    syscall(number, args)
}

fn call(number: abi::Sys, a0: usize, a1: usize, a2: usize) -> isize {
    syscall(number, [a0, a1, a2, 0, 0, 0])
}

pub fn fork() -> i32 {
    call(abi::Sys::Fork, 0, 0, 0) as i32
}

pub fn exit(status: i32) -> ! {
    let _ = call(abi::Sys::Exit, status as usize, 0, 0);
    loop {
        core::hint::spin_loop();
    }
}

pub fn wait(status: Option<&mut i32>) -> i32 {
    let address = status.map_or(0, |status| status as *mut i32 as usize);
    call(abi::Sys::Wait, address, 0, 0) as i32
}

pub fn pipe(fds: &mut [i32; 2]) -> i32 {
    call(abi::Sys::Pipe, fds.as_mut_ptr() as usize, 0, 0) as i32
}

pub fn read(fd: i32, dst: &mut [u8]) -> isize {
    call(
        abi::Sys::Read,
        fd as usize,
        dst.as_mut_ptr() as usize,
        dst.len(),
    )
}

pub fn write(fd: i32, src: &[u8]) -> isize {
    call(
        abi::Sys::Write,
        fd as usize,
        src.as_ptr() as usize,
        src.len(),
    )
}

pub fn close(fd: i32) -> i32 {
    call(abi::Sys::Close, fd as usize, 0, 0) as i32
}

pub fn kill(pid: i32) -> i32 {
    call(abi::Sys::Kill, pid as usize, 0, 0) as i32
}

pub fn getpid() -> i32 {
    call(abi::Sys::Getpid, 0, 0, 0) as i32
}

pub fn sbrk(bytes: isize) -> isize {
    call(abi::Sys::Sbrk, bytes as usize, abi::sbrk::EAGER, 0)
}

pub fn sbrklazy(bytes: isize) -> isize {
    call(abi::Sys::Sbrk, bytes as usize, abi::sbrk::LAZY, 0)
}

pub fn pause(ticks: i32) -> i32 {
    call(abi::Sys::Pause, ticks as usize, 0, 0) as i32
}

pub fn uptime() -> usize {
    call(abi::Sys::Uptime, 0, 0, 0) as usize
}

pub fn dup(fd: i32) -> i32 {
    call(abi::Sys::Dup, fd as usize, 0, 0) as i32
}

pub fn open(path: &[u8], mode: i32) -> i32 {
    let Some(path) = c_path(path) else { return -1 };
    call(abi::Sys::Open, path.as_ptr() as usize, mode as usize, 0) as i32
}

pub fn mknod(path: &[u8], major: i16, minor: i16) -> i32 {
    let Some(path) = c_path(path) else { return -1 };
    call(
        abi::Sys::Mknod,
        path.as_ptr() as usize,
        major as usize,
        minor as usize,
    ) as i32
}

pub fn chdir(path: &[u8]) -> i32 {
    let Some(path) = c_path(path) else { return -1 };
    call(abi::Sys::Chdir, path.as_ptr() as usize, 0, 0) as i32
}

pub fn link(old: &[u8], new: &[u8]) -> i32 {
    let Some(old) = c_path(old) else { return -1 };
    let Some(new) = c_path(new) else { return -1 };
    call(
        abi::Sys::Link,
        old.as_ptr() as usize,
        new.as_ptr() as usize,
        0,
    ) as i32
}

pub fn unlink(path: &[u8]) -> i32 {
    let Some(path) = c_path(path) else {
        return -1;
    };
    call(abi::Sys::Unlink, path.as_ptr() as usize, 0, 0) as i32
}

pub fn mkdir(path: &[u8]) -> i32 {
    let Some(path) = c_path(path) else {
        return -1;
    };
    call(abi::Sys::Mkdir, path.as_ptr() as usize, 0, 0) as i32
}

pub fn sync() -> i32 {
    call(abi::Sys::Sync, 0, 0, 0) as i32
}

pub fn fstat(fd: i32) -> Result<Stat, Error> {
    let mut stat = core::mem::MaybeUninit::<Stat>::uninit();
    if call(abi::Sys::Fstat, fd as usize, stat.as_mut_ptr() as usize, 0) < 0 {
        return Err(Error);
    }
    // SAFETY: successful fstat initializes every byte of the ABI Stat.
    Ok(unsafe { stat.assume_init() })
}

pub fn stat(path: &[u8]) -> Result<Stat, Error> {
    let fd = open(path, abi::fcntl::O_RDONLY);
    if fd < 0 {
        return Err(Error);
    }
    let result = fstat(fd);
    let _ = close(fd);
    result
}

pub fn exec(path: &[u8], args: &[&[u8]]) -> isize {
    if args.len() >= MAXARG {
        return -1;
    }
    let mut path = path.to_vec();
    if path.last() != Some(&0) {
        path.push(0);
    }
    if path.len() > MAXPATH {
        return -1;
    }
    let mut strings: Vec<Vec<u8>> = Vec::with_capacity(args.len());
    for arg in args {
        let mut string = arg.to_vec();
        if string.last() != Some(&0) {
            string.push(0);
        }
        strings.push(string);
    }
    let mut pointers: Vec<*const u8> = strings.iter().map(|arg| arg.as_ptr()).collect();
    // The kernel walks until this explicit null pointer.
    pointers.push(ptr::null());
    call(
        abi::Sys::Exec,
        path.as_ptr() as usize,
        pointers.as_ptr() as usize,
        0,
    )
}

pub fn gets(dst: &mut [u8]) -> usize {
    let mut used = 0;
    while used < dst.len() {
        let read_count = read(0, &mut dst[used..used + 1]);
        if read_count != 1 {
            break;
        }
        used += 1;
        if dst[used - 1] == b'\n' {
            break;
        }
    }
    used
}

fn c_path(path: &[u8]) -> Option<[u8; MAXPATH]> {
    let len = path
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(path.len());
    if len + 1 > MAXPATH {
        return None;
    }
    let mut result = [0; MAXPATH];
    result[..len].copy_from_slice(&path[..len]);
    Some(result)
}

pub fn atoi(bytes: &[u8]) -> i32 {
    let bytes = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(&[][..], |start| &bytes[start..]);
    let (negative, digits) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        Some(b'+') => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    let mut value = 0i64;
    for byte in digits {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(*byte - b'0'));
    }
    let signed = if negative {
        value.saturating_neg()
    } else {
        value
    };
    signed.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

pub struct Stdout;

impl fmt::Write for Stdout {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let _ = write(1, text.as_bytes());
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
    let _ = Stdout.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{ $crate::_print(core::format_args!($($arg)*)); }};
}

#[macro_export]
macro_rules! println {
    () => {{ $crate::print!("\n"); }};
    ($($arg:tt)*) => {{ $crate::_print(core::format_args!("{}\n", core::format_args!($($arg)*))); }};
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    _print(format_args!("panic: {info}\n"));
    exit(1)
}

#[repr(C)]
struct Header {
    next: *mut Header,
    size: usize,
}

struct AllocState {
    base: Header,
    freep: *mut Header,
}

struct Allocator {
    locked: AtomicBool,
    state: UnsafeCell<AllocState>,
}

// SAFETY: `state` is accessed only while `locked` is held.
unsafe impl Sync for Allocator {}

impl Allocator {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            state: UnsafeCell::new(AllocState {
                base: Header {
                    next: ptr::null_mut(),
                    size: 0,
                },
                freep: ptr::null_mut(),
            }),
        }
    }

    fn lock(&self) -> AllocGuard<'_> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        AllocGuard { allocator: self }
    }
}

struct AllocGuard<'a> {
    allocator: &'a Allocator,
}

impl AllocGuard<'_> {
    fn state(&mut self) -> &mut AllocState {
        // SAFETY: this guard holds the allocator's spin lock exclusively.
        unsafe { &mut *self.allocator.state.get() }
    }
}

impl Drop for AllocGuard<'_> {
    fn drop(&mut self) {
        self.allocator.locked.store(false, Ordering::Release);
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator::new();

// SAFETY: allocation metadata is serialized by `Allocator::lock`; blocks
// come from the process's monotonically grown `sbrk` heap and are returned
// only through this same allocator.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }
        if layout.align() > align_of::<Header>() {
            return ptr::null_mut();
        }
        let mut guard = self.lock();
        // SAFETY: the allocator lock gives exclusive metadata access.
        unsafe { guard.state().alloc(layout.size()) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
        if pointer.is_null() {
            return;
        }
        let mut guard = self.lock();
        // SAFETY: GlobalAlloc guarantees `pointer` came from this allocator.
        unsafe { guard.state().free(pointer) };
    }
}

impl AllocState {
    unsafe fn alloc(&mut self, bytes: usize) -> *mut u8 {
        let units = bytes.div_ceil(size_of::<Header>()) + 1;
        if self.freep.is_null() {
            let base = ptr::addr_of_mut!(self.base);
            self.base.next = base;
            self.freep = base;
        }
        let mut previous = self.freep;
        // SAFETY: initialized free-list links form a circular list.
        let mut current = unsafe { (*previous).next };
        loop {
            // SAFETY: `current` is a live free-list header.
            if unsafe { (*current).size } >= units {
                // SAFETY: allocator lock owns both headers.
                if unsafe { (*current).size } == units {
                    // SAFETY: both headers belong to the locked free list.
                    unsafe { (*previous).next = (*current).next };
                } else {
                    // SAFETY: the selected free block is large enough to
                    // split, and the allocator lock excludes other access.
                    unsafe {
                        (*current).size -= units;
                        current = current.add((*current).size);
                        (*current).size = units;
                    }
                }
                self.freep = previous;
                // SAFETY: one header follows the allocation payload start.
                return unsafe { current.add(1).cast() };
            }
            if current == self.freep {
                // SAFETY: `morecore` inserts a new free block or fails.
                current = unsafe { self.morecore(units) };
                if current.is_null() {
                    return ptr::null_mut();
                }
            }
            previous = current;
            // SAFETY: circular free-list link.
            current = unsafe { (*current).next };
        }
    }

    unsafe fn morecore(&mut self, units: usize) -> *mut Header {
        let units = units.max(4096);
        let bytes = match units.checked_mul(size_of::<Header>()) {
            Some(bytes) => bytes,
            None => return ptr::null_mut(),
        };
        let address = sbrk(bytes as isize);
        if address < 0 {
            return ptr::null_mut();
        }
        let block = address as usize as *mut Header;
        // SAFETY: sbrk returned a fresh `bytes`-long aligned heap range.
        unsafe {
            (*block).size = units;
            self.free(block.add(1).cast());
        }
        self.freep
    }

    unsafe fn free(&mut self, pointer: *mut u8) {
        // SAFETY: allocation payloads have exactly one preceding Header.
        let block = unsafe { pointer.cast::<Header>().sub(1) };
        let mut current = self.freep;
        loop {
            // SAFETY: current is a live free-list header.
            let next = unsafe { (*current).next };
            if (block > current && block < next)
                || (current >= next && (block > current || block < next))
            {
                break;
            }
            current = next;
        }
        // SAFETY: allocator lock owns the adjacent metadata.
        unsafe {
            if block.add((*block).size) == (*current).next {
                (*block).size += (*(*current).next).size;
                (*block).next = (*(*current).next).next;
            } else {
                (*block).next = (*current).next;
            }
            if current.add((*current).size) == block {
                (*current).size += (*block).size;
                (*current).next = (*block).next;
            } else {
                (*current).next = block;
            }
        }
        self.freep = current;
    }
}
