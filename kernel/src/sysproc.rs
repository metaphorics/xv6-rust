//! Process system calls (`kernel/sysproc.c`).

use crate::err::Err;
use crate::proc::{self, CurrentProc};
use crate::syscall::{arg_addr, arg_int};
use crate::trap::TICKS;

/// `sys_exit` (sysproc.c:10-17): exit with a status; never returns.
pub fn exit(p: &CurrentProc) -> ! {
    let n = arg_int(p, 0);
    proc::exit(n)
}

/// `sys_fork` (sysproc.c:26-29): the child runs as a copy of the caller.
pub fn fork() -> Result<usize, Err> {
    proc::fork()
}

/// `sys_wait` (sysproc.c:31-37): reap a child; the optional user
/// address receives its exit status.
pub fn wait(p: &CurrentProc) -> Result<usize, Err> {
    let addr = arg_addr(p, 0);
    proc::wait(addr)
}

/// `sys_kill` (sysproc.c:92-99).
pub fn kill(p: &CurrentProc) -> Result<usize, Err> {
    let pid = arg_int(p, 0);
    proc::kill(pid)?;
    Ok(0)
}

/// `sys_getpid` (sysproc.c:20-23).
pub fn getpid(p: &CurrentProc) -> Result<usize, Err> {
    Ok(p.pid() as usize)
}

/// `sys_sbrk` (sysproc.c:39-65): grow (or shrink) the caller's memory
/// by `n` bytes, returning the old break. Upstream xv6's eager form:
/// this reference's lazy variant (grow the size, fault pages in later)
/// needs the `vmfault` machinery that joins with the usertests port,
/// so until then every growth is real memory and every fault is fatal.
pub fn sbrk(p: &CurrentProc) -> Result<usize, Err> {
    let n = arg_int(p, 0) as i64;
    let addr = p.sz();
    proc::grow(n)?;
    Ok(addr as usize)
}

/// `sys_pause` (sysproc.c:67-90): sleep for `n` timer ticks, checking
/// `killed` each round. Negative counts clamp to zero, as in C.
pub fn pause(p: &CurrentProc) -> Result<usize, Err> {
    let mut n = arg_int(p, 0);
    if n < 0 {
        n = 0;
    }
    let n = n as u64;

    let mut ticks = TICKS.lock();
    let ticks0 = *ticks;
    while *ticks - ticks0 < n {
        if p.killed() {
            return Err(Err::BadArg);
        }
        ticks = proc::sleep(TICKS.chan(), ticks);
    }
    Ok(0)
}

/// `sys_uptime` (sysproc.c:103-112): clock ticks since boot.
pub fn uptime() -> Result<usize, Err> {
    let ticks = TICKS.lock();
    Ok(*ticks as usize)
}
