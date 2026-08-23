//! x86_64 callee-saved kernel context switch.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Context {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rsp: u64,
}

const _: () = assert!(core::mem::size_of::<Context>() == 56);

impl Context {
    pub const ZERO: Context = Context {
        r15: 0,
        r14: 0,
        r13: 0,
        r12: 0,
        rbx: 0,
        rbp: 0,
        rsp: 0,
    };

    pub fn new(entry: u64, stack_top: u64) -> Context {
        let rsp = stack_top - 16;
        // SAFETY: allocproc calls this only after this process's private kernel
        // stack has been mapped; the top word is reserved as the initial return.
        unsafe { (rsp as *mut u64).write(entry) };
        Context {
            rsp,
            ..Context::ZERO
        }
    }
}

core::arch::global_asm!(
    ".section .text,\"ax\"",
    ".globl swtch",
    "swtch:",
    "    movq %r15, 0(%rdi)",
    "    movq %r14, 8(%rdi)",
    "    movq %r13, 16(%rdi)",
    "    movq %r12, 24(%rdi)",
    "    movq %rbx, 32(%rdi)",
    "    movq %rbp, 40(%rdi)",
    "    movq %rsp, 48(%rdi)",
    "    movq 0(%rsi), %r15",
    "    movq 8(%rsi), %r14",
    "    movq 16(%rsi), %r13",
    "    movq 24(%rsi), %r12",
    "    movq 32(%rsi), %rbx",
    "    movq 40(%rsi), %rbp",
    "    movq 48(%rsi), %rsp",
    "    ret",
    options(att_syntax)
);

unsafe extern "C" {
    fn swtch(old: *mut Context, new: *const Context);
}

/// # Safety
/// Both contexts must remain live and distinct, and interrupts must be off.
pub unsafe fn switch(old: *mut Context, new: *const Context) {
    // SAFETY: upheld by the scheduler's context ownership discipline.
    unsafe { swtch(old, new) }
}
