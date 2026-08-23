//! Syscall dispatch and argument unpacking (`kernel/syscall.c`).

use crate::dev::console;
use crate::err::Err;
use crate::proc::CurrentProc;
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
        Sys::Fork => sysproc::fork(),
        Sys::Exit => sysproc::exit(p),
        Sys::Wait => sysproc::wait(p),
        Sys::Kill => sysproc::kill(p),
        Sys::Getpid => sysproc::getpid(p),
        Sys::Sbrk => sysproc::sbrk(p),
        Sys::Pause => sysproc::pause(p),
        Sys::Uptime => sysproc::uptime(),
        Sys::Write => sys_write(p),
        // The remaining calls belong to the file-system and pipe layers
        // (M5-M7); until those land they fail like any bad argument,
        // and nothing in the boot path issues them.
        _ => Err(Err::BadArg),
    };

    p.trapframe().set_ret(match ret {
        Ok(value) => value as u64,
        // The C handlers' -1 (syscall.c:146).
        Err(_) => u64::MAX,
    });
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

/// `sys_write` for this milestone (`sysfile.c`'s filewrite plus
/// console.c's consolewrite): write `n` bytes from the user buffer to
/// the console for fds 1 and 2.
///
/// Layering note: the file table is M5; until then this is the only
/// write path, and it is exactly the path the file table will route
/// fds 1 and 2 to (`File::Device(CONSOLE)` → `console::write`). M5's
/// commit deletes this special case in favor of the table.
fn sys_write(p: &CurrentProc) -> Result<usize, Err> {
    let fd = arg_int(p, 0);
    let buf = arg_addr(p, 1);
    let n = arg_int(p, 2);
    if n < 0 {
        return Err(Err::BadArg);
    }
    if fd != 1 && fd != 2 {
        return Err(Err::BadArg);
    }
    Ok(console::write(true, buf, n as usize))
}
