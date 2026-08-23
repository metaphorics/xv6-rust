//! Syscall dispatch and argument unpacking (`kernel/syscall.c`).

use crate::err::Err;
use crate::proc::CurrentProc;
use crate::sysfile;
use crate::sysproc;
use abi::Sys;

/// Dispatch the current syscall: read the number from a7, run the
/// handler, store its result in a0 (`syscall`, syscall.c:136-150).
pub fn dispatch(p: &CurrentProc) {
    let num = p.trapframe().syscall_num();
    let Ok(sys) = Sys::try_from(num) else {
        println!("{} {}: unknown sys call {}", p.pid(), p.name_str(), num);
        p.trapframe().set_ret(u64::MAX);
        return;
    };

    let ret: Result<usize, Err> = match sys {
        Sys::Pipe => sysfile::pipe(p),
        Sys::Fork => sysproc::fork(),
        Sys::Exit => sysproc::exit(p),
        Sys::Wait => sysproc::wait(p),
        Sys::Kill => sysproc::kill(p),
        Sys::Getpid => sysproc::getpid(p),
        Sys::Sbrk => sysproc::sbrk(p),
        Sys::Pause => sysproc::pause(p),
        Sys::Uptime => sysproc::uptime(),
        Sys::Exec => sysfile::exec(p),
        Sys::Fstat => sysfile::fstat(p),
        Sys::Link => sysfile::link(p),
        Sys::Unlink => sysfile::unlink(p),
        Sys::Mkdir => sysfile::mkdir(p),
        Sys::Chdir => sysfile::chdir(p),
        Sys::Dup => sysfile::dup(p),
        Sys::Open => sysfile::open(p),
        Sys::Write => sysfile::write(p),
        Sys::Mknod => sysfile::mknod(p),
        Sys::Close => sysfile::close(p),
        Sys::Sync => sysfile::sync(p),
        Sys::Read => sysfile::read(p),
        // Every ABI syscall has a concrete handler by M7.
    };

    let succeeded = ret.is_ok();
    let value = match ret {
        Ok(value) => value as u64,
        // The C handlers' -1 (syscall.c:146).
        Err(_) => u64::MAX,
    };
    if succeeded && matches!(sys, Sys::Exec) {
        p.trapframe().set_entry_arg(value);
    }
    p.trapframe().set_ret(value);
}

/// Fetch the nth word-sized syscall argument (`argraw`, syscall.c:35-54):
/// a0 through a5. C panics past 5; the accessor does.
fn arg_raw(p: &CurrentProc, n: usize) -> u64 {
    p.trapframe().arg(n)
}

/// Fetch the nth 32-bit syscall argument (`argint`, syscall.c:57-61) —
/// the u64 register truncated, exactly the C store.
pub fn arg_int(p: &CurrentProc, n: usize) -> i32 {
    arg_raw(p, n) as i32
}

/// Retrieve the nth argument as a pointer (`argaddr`, syscall.c:63-70).
/// Legality is copyin/copyout's to check, as in C.
pub fn arg_addr(p: &CurrentProc, n: usize) -> u64 {
    arg_raw(p, n)
}
