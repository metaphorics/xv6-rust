#![no_std]
#![forbid(unsafe_code)]

//! Shared ABI definitions for the xv6-rust rewrite.
//!
//! Everything here is used by more than one of the kernel, the user
//! runtime, and the `mkfs` host tool: syscall numbers, the `stat` layout,
//! open flags, and (from M5) the on-disk file-system types with explicit
//! little-endian codecs.

/// Error returned when a number is not a valid syscall number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownSyscall(pub u64);

impl core::fmt::Display for UnknownSyscall {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "unknown syscall number {}", self.0)
    }
}

/// xv6 system call numbers, in `kernel/syscall.h` order.
///
/// Values verified against `.references/xv6-riscv/kernel/syscall.h`, whose
/// syscall wrapper list `user/usys.pl` mirrors. This reference tree differs from
/// upstream xv6: syscall 13 is named `pause` (upstream's `sleep`), and
/// `sync` (22) was added for the crash-recovery tests, so the surface is
/// 22 calls rather than upstream's 21.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sys {
    Fork = 1,
    Exit = 2,
    Wait = 3,
    Pipe = 4,
    Read = 5,
    Kill = 6,
    Exec = 7,
    Fstat = 8,
    Chdir = 9,
    Dup = 10,
    Getpid = 11,
    Sbrk = 12,
    Pause = 13,
    Uptime = 14,
    Open = 15,
    Write = 16,
    Mknod = 17,
    Unlink = 18,
    Link = 19,
    Mkdir = 20,
    Close = 21,
    Sync = 22,
}

impl Sys {
    /// The xv6 name of this syscall, as used by `usys.pl` and `syscall.c`.
    pub const fn name(self) -> &'static str {
        match self {
            Sys::Fork => "fork",
            Sys::Exit => "exit",
            Sys::Wait => "wait",
            Sys::Pipe => "pipe",
            Sys::Read => "read",
            Sys::Kill => "kill",
            Sys::Exec => "exec",
            Sys::Fstat => "fstat",
            Sys::Chdir => "chdir",
            Sys::Dup => "dup",
            Sys::Getpid => "getpid",
            Sys::Sbrk => "sbrk",
            Sys::Pause => "pause",
            Sys::Uptime => "uptime",
            Sys::Open => "open",
            Sys::Write => "write",
            Sys::Mknod => "mknod",
            Sys::Unlink => "unlink",
            Sys::Link => "link",
            Sys::Mkdir => "mkdir",
            Sys::Close => "close",
            Sys::Sync => "sync",
        }
    }
}

impl TryFrom<u64> for Sys {
    type Error = UnknownSyscall;

    fn try_from(num: u64) -> Result<Self, Self::Error> {
        match num {
            1 => Ok(Sys::Fork),
            2 => Ok(Sys::Exit),
            3 => Ok(Sys::Wait),
            4 => Ok(Sys::Pipe),
            5 => Ok(Sys::Read),
            6 => Ok(Sys::Kill),
            7 => Ok(Sys::Exec),
            8 => Ok(Sys::Fstat),
            9 => Ok(Sys::Chdir),
            10 => Ok(Sys::Dup),
            11 => Ok(Sys::Getpid),
            12 => Ok(Sys::Sbrk),
            13 => Ok(Sys::Pause),
            14 => Ok(Sys::Uptime),
            15 => Ok(Sys::Open),
            16 => Ok(Sys::Write),
            17 => Ok(Sys::Mknod),
            18 => Ok(Sys::Unlink),
            19 => Ok(Sys::Link),
            20 => Ok(Sys::Mkdir),
            21 => Ok(Sys::Close),
            22 => Ok(Sys::Sync),
            _ => Err(UnknownSyscall(num)),
        }
    }
}

/// Open flags shared by kernel and userland (`kernel/fcntl.h`).
pub mod fcntl {
    pub const O_RDONLY: i32 = 0x000;
    pub const O_WRONLY: i32 = 0x001;
    pub const O_RDWR: i32 = 0x002;
    pub const O_CREATE: i32 = 0x200;
    pub const O_TRUNC: i32 = 0x400;
}

/// File types (`T_DIR`, `T_FILE`, `T_DEVICE` in `kernel/stat.h`).
#[repr(i16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileType {
    Dir = 1,
    File = 2,
    Device = 3,
}

/// `struct stat` as reported by `fstat` (`kernel/stat.h`).
///
/// Field order and widths match the C layout exactly: `repr(C)` gives
/// dev:0, ino:4, type:8, nlink:10, size:16, for 24 bytes on both riscv64
/// and x86_64. The layout is pinned by test.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub dev: i32,
    pub ino: u32,
    pub r#type: i16,
    pub nlink: i16,
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::{FileType, Stat, Sys, UnknownSyscall, fcntl};

    /// Every syscall, in `kernel/syscall.h` order.
    const ALL: [Sys; 22] = [
        Sys::Fork,
        Sys::Exit,
        Sys::Wait,
        Sys::Pipe,
        Sys::Read,
        Sys::Kill,
        Sys::Exec,
        Sys::Fstat,
        Sys::Chdir,
        Sys::Dup,
        Sys::Getpid,
        Sys::Sbrk,
        Sys::Pause,
        Sys::Uptime,
        Sys::Open,
        Sys::Write,
        Sys::Mknod,
        Sys::Unlink,
        Sys::Link,
        Sys::Mkdir,
        Sys::Close,
        Sys::Sync,
    ];

    #[test]
    fn syscall_numbers_round_trip_in_header_order() {
        for (index, sys) in ALL.iter().enumerate() {
            let num = u64::from(*sys as u16);
            assert_eq!(Sys::try_from(num), Ok(*sys));
            // The numbers are contiguous from 1, matching the C header.
            assert_eq!(num, index as u64 + 1, "{sys:?} is out of header order");
            assert!(!sys.name().is_empty());
        }
    }

    #[test]
    fn unknown_syscall_numbers_are_rejected() {
        assert_eq!(Sys::try_from(0), Err(UnknownSyscall(0)));
        // 22 is Sys::Sync in this reference tree; 23 is the first free slot.
        assert_eq!(Sys::try_from(23), Err(UnknownSyscall(23)));
        assert_eq!(Sys::try_from(u64::MAX), Err(UnknownSyscall(u64::MAX)));
    }

    #[test]
    fn stat_layout_matches_the_c_struct() {
        assert_eq!(core::mem::size_of::<Stat>(), 24);
        assert_eq!(core::mem::align_of::<Stat>(), 8);
        assert_eq!(core::mem::offset_of!(Stat, dev), 0);
        assert_eq!(core::mem::offset_of!(Stat, ino), 4);
        assert_eq!(core::mem::offset_of!(Stat, r#type), 8);
        assert_eq!(core::mem::offset_of!(Stat, nlink), 10);
        assert_eq!(core::mem::offset_of!(Stat, size), 16);
        assert_eq!(FileType::Dir as i16, 1);
        assert_eq!(FileType::File as i16, 2);
        assert_eq!(FileType::Device as i16, 3);
    }

    #[test]
    fn fcntl_flags_match_the_c_header() {
        assert_eq!(fcntl::O_RDONLY, 0x000);
        assert_eq!(fcntl::O_WRONLY, 0x001);
        assert_eq!(fcntl::O_RDWR, 0x002);
        assert_eq!(fcntl::O_CREATE, 0x200);
        assert_eq!(fcntl::O_TRUNC, 0x400);
    }
}
