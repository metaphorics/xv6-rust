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
/// Allocation modes for the two-argument `sbrk` syscall (`kernel/vm.h`).
pub mod sbrk {
    pub const EAGER: usize = 1;
    pub const LAZY: usize = 2;
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

/// File-system block size (`kernel/fs.h:4`).
pub const BSIZE: usize = 1_024;
/// Root inode number (`kernel/fs.h:3`).
pub const ROOTINO: u32 = 1;
/// Root disk device (`kernel/param.h:7`).
pub const ROOTDEV: u32 = 1;
/// File-system magic (`kernel/fs.h:20`).
pub const FSMAGIC: u32 = 0x1020_3040;
/// Direct block pointers in an inode (`kernel/fs.h:22`).
pub const NDIRECT: usize = 12;
/// Indirect pointers per block (`kernel/fs.h:23`).
pub const NINDIRECT: usize = BSIZE / core::mem::size_of::<u32>();
/// Maximum file size in blocks (`kernel/fs.h:24`).
pub const MAXFILE: usize = NDIRECT + NINDIRECT;
/// Directory-name bytes (`kernel/fs.h:40`).
pub const DIRSIZ: usize = 14;
/// Total blocks in an image (`kernel/param.h:12`).
pub const FSSIZE: u32 = 2_000;
/// Inodes created by mkfs (`mkfs/mkfs.c:23`).
pub const NINODES: u32 = 200;
/// Maximum blocks one operation may dirty (`kernel/param.h:9`).
pub const MAXOPBLOCKS: usize = 10;
/// Data blocks reserved for the write-ahead log (`kernel/param.h:10`).
pub const LOGBLOCKS: usize = MAXOPBLOCKS * 3;
/// Encoded dinodes per block (`kernel/fs.h:36`).
pub const IPB: u32 = (BSIZE / Dinode::ENCODED_LEN) as u32;
/// Allocation bits per bitmap block (`kernel/fs.h:45`).
pub const BPB: u32 = (BSIZE * 8) as u32;

/// Disk block containing inode `inum` (`IBLOCK`, kernel/fs.h:37).
pub const fn inode_block(inum: u32, inodestart: u32) -> u32 {
    inum / IPB + inodestart
}

/// Bitmap block containing the allocation bit for `blockno`
/// (`BBLOCK`, kernel/fs.h:48).
pub const fn bitmap_block(blockno: u32, bmapstart: u32) -> u32 {
    blockno / BPB + bmapstart
}

fn get_u16(src: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(src.get(at..at + 2)?.try_into().ok()?))
}

fn get_i16(src: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(src.get(at..at + 2)?.try_into().ok()?))
}

fn get_u32(src: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(src.get(at..at + 4)?.try_into().ok()?))
}

/// On-disk superblock (`struct superblock`, kernel/fs.h:8-17).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Superblock {
    pub magic: u32,
    pub size: u32,
    pub nblocks: u32,
    pub ninodes: u32,
    pub nlog: u32,
    pub logstart: u32,
    pub inodestart: u32,
    pub bmapstart: u32,
}

impl Superblock {
    pub const ENCODED_LEN: usize = 32;

    pub fn encode(self) -> [u8; Self::ENCODED_LEN] {
        let mut dst = [0; Self::ENCODED_LEN];
        for (index, value) in [
            self.magic,
            self.size,
            self.nblocks,
            self.ninodes,
            self.nlog,
            self.logstart,
            self.inodestart,
            self.bmapstart,
        ]
        .iter()
        .enumerate()
        {
            dst[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        dst
    }

    pub fn decode(src: &[u8]) -> Option<Self> {
        Some(Self {
            magic: get_u32(src, 0)?,
            size: get_u32(src, 4)?,
            nblocks: get_u32(src, 8)?,
            ninodes: get_u32(src, 12)?,
            nlog: get_u32(src, 16)?,
            logstart: get_u32(src, 20)?,
            inodestart: get_u32(src, 24)?,
            bmapstart: get_u32(src, 28)?,
        })
    }
}

/// On-disk inode (`struct dinode`, kernel/fs.h:27-34).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Dinode {
    pub r#type: i16,
    pub major: i16,
    pub minor: i16,
    pub nlink: i16,
    pub size: u32,
    pub addrs: [u32; NDIRECT + 1],
}

impl Dinode {
    pub const ENCODED_LEN: usize = 64;

    pub fn encode(self) -> [u8; Self::ENCODED_LEN] {
        let mut dst = [0; Self::ENCODED_LEN];
        for (at, value) in [
            (0, self.r#type),
            (2, self.major),
            (4, self.minor),
            (6, self.nlink),
        ] {
            dst[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        dst[8..12].copy_from_slice(&self.size.to_le_bytes());
        for (index, addr) in self.addrs.iter().enumerate() {
            let at = 12 + index * 4;
            dst[at..at + 4].copy_from_slice(&addr.to_le_bytes());
        }
        dst
    }

    pub fn decode(src: &[u8]) -> Option<Self> {
        let mut addrs = [0; NDIRECT + 1];
        for (index, addr) in addrs.iter_mut().enumerate() {
            *addr = get_u32(src, 12 + index * 4)?;
        }
        Some(Self {
            r#type: get_i16(src, 0)?,
            major: get_i16(src, 2)?,
            minor: get_i16(src, 4)?,
            nlink: get_i16(src, 6)?,
            size: get_u32(src, 8)?,
            addrs,
        })
    }
}

/// On-disk directory entry (`struct dirent`, kernel/fs.h:42-43).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Dirent {
    pub inum: u16,
    pub name: [u8; DIRSIZ],
}

impl Dirent {
    pub const ENCODED_LEN: usize = 16;

    pub fn new(inum: u16, name: &[u8]) -> Option<Self> {
        if name.len() > DIRSIZ {
            return None;
        }
        let mut entry = Self {
            inum,
            name: [0; DIRSIZ],
        };
        entry.name[..name.len()].copy_from_slice(name);
        Some(entry)
    }

    pub fn encode(self) -> [u8; Self::ENCODED_LEN] {
        let mut dst = [0; Self::ENCODED_LEN];
        dst[..2].copy_from_slice(&self.inum.to_le_bytes());
        dst[2..].copy_from_slice(&self.name);
        dst
    }

    pub fn decode(src: &[u8]) -> Option<Self> {
        let mut name = [0; DIRSIZ];
        name.copy_from_slice(src.get(2..Self::ENCODED_LEN)?);
        Some(Self {
            inum: get_u16(src, 0)?,
            name,
        })
    }
}

/// In-memory representation of the on-disk log header
/// (`struct logheader`, kernel/log.c:22-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogHeader {
    pub n: u32,
    pub block: [u32; LOGBLOCKS],
}

impl Default for LogHeader {
    fn default() -> Self {
        Self {
            n: 0,
            block: [0; LOGBLOCKS],
        }
    }
}

impl LogHeader {
    pub const ENCODED_LEN: usize = 4 + LOGBLOCKS * 4;

    /// Encode into a complete on-disk header block. The unused tail is zero.
    pub fn encode_block(self) -> [u8; BSIZE] {
        let mut dst = [0; BSIZE];
        dst[..4].copy_from_slice(&self.n.to_le_bytes());
        for (index, block) in self.block.iter().enumerate() {
            let at = 4 + index * 4;
            dst[at..at + 4].copy_from_slice(&block.to_le_bytes());
        }
        dst
    }

    pub fn decode_block(src: &[u8]) -> Option<Self> {
        if src.len() < BSIZE {
            return None;
        }
        let n = get_u32(src, 0)?;
        if n as usize > LOGBLOCKS {
            return None;
        }
        let mut block = [0; LOGBLOCKS];
        for (index, value) in block.iter_mut().enumerate() {
            *value = get_u32(src, 4 + index * 4)?;
        }
        Some(Self { n, block })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BPB, BSIZE, DIRSIZ, Dinode, Dirent, FSMAGIC, FileType, IPB, LOGBLOCKS, LogHeader, NDIRECT,
        Stat, Superblock, Sys, UnknownSyscall, bitmap_block, fcntl, inode_block,
    };

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
    fn syscall_surface_is_22_calls_ending_in_sync() {
        assert_eq!(ALL.len(), 22);
        assert_eq!(Sys::Sync as u64, 22);
    }

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

    #[test]
    fn on_disk_codecs_round_trip() {
        let sb = Superblock {
            magic: FSMAGIC,
            size: 2_000,
            nblocks: 1_953,
            ninodes: 200,
            nlog: 31,
            logstart: 2,
            inodestart: 33,
            bmapstart: 46,
        };
        assert_eq!(Superblock::decode(&sb.encode()), Some(sb));

        let inode = Dinode {
            r#type: FileType::File as i16,
            major: 4,
            minor: 7,
            nlink: 2,
            size: 0x1234_5678,
            addrs: core::array::from_fn(|index| index as u32 * 17),
        };
        assert_eq!(Dinode::decode(&inode.encode()), Some(inode));

        let entry = Dirent::new(42, b"hello").expect("valid dirent");
        assert_eq!(Dirent::decode(&entry.encode()), Some(entry));

        let mut header = LogHeader {
            n: LOGBLOCKS as u32,
            ..LogHeader::default()
        };
        header.block = core::array::from_fn(|index| 100 + index as u32);
        assert_eq!(
            LogHeader::decode_block(&header.encode_block()),
            Some(header)
        );
    }

    #[test]
    fn on_disk_layout_and_block_helpers_match_c() {
        assert_eq!(core::mem::size_of::<Superblock>(), 32);
        assert_eq!(core::mem::size_of::<Dinode>(), 64);
        assert_eq!(core::mem::size_of::<Dirent>(), 16);
        assert_eq!(Superblock::ENCODED_LEN, 32);
        assert_eq!(Dinode::ENCODED_LEN, 64);
        assert_eq!(Dirent::ENCODED_LEN, 16);
        assert_eq!(BSIZE, 1_024);
        assert_eq!(DIRSIZ, 14);
        assert_eq!(NDIRECT, 12);
        assert_eq!(IPB, 16);
        assert_eq!(BPB, 8_192);
        assert_eq!(inode_block(17, 33), 34);
        assert_eq!(bitmap_block(8_192, 46), 47);
    }
}
