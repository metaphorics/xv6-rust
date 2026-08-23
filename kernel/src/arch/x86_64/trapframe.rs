//! Saved x86_64 user register state.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TrapFrame {
    pub kernel_sp: u64,
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub epc: u64,
    pub rflags: u64,
    pub sp: u64,
}

const _: () = assert!(core::mem::size_of::<TrapFrame>() == 152);

impl TrapFrame {
    pub fn syscall_num(&self) -> u64 {
        self.rax
    }

    pub fn arg(&self, n: usize) -> u64 {
        match n {
            0 => self.rdi,
            1 => self.rsi,
            2 => self.rdx,
            3 => self.r10,
            4 => self.r8,
            5 => self.r9,
            _ => panic!("argraw"),
        }
    }

    pub fn set_ret(&mut self, value: u64) {
        self.rax = value;
    }

    pub fn set_exec(&mut self, entry: u64, sp: u64, argv: u64) {
        self.epc = entry;
        self.sp = sp - 8;
        self.rsi = argv;
        self.rflags = 0x202;
    }

    pub fn set_entry_arg(&mut self, value: u64) {
        self.rdi = value;
    }
}
