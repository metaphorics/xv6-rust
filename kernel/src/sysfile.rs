//! File-system syscall plumbing (`kernel/sysfile.c`).

use abi::fcntl::{O_CREATE, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
use abi::{FileType, Stat};

use crate::arch::PAGE_SIZE;
use crate::err::Err;
use crate::exec;
use crate::fs::file::FileHandle;
use crate::fs::inode::{self, Inode};
use crate::fs::log;
use crate::mm::uvm;
use crate::params::{MAXARG, MAXPATH, NDEV, NOFILE};
use crate::proc::CurrentProc;
use crate::syscall::{arg_addr, arg_int};

fn arg_fd(process: &CurrentProc, n: usize) -> Result<(usize, FileHandle), Err> {
    let fd = arg_int(process, n);
    let fd = usize::try_from(fd).map_err(|_| Err::BadArg)?;
    let file = process.file(fd).ok_or(Err::BadArg)?;
    Ok((fd, file))
}

fn fd_alloc(process: &CurrentProc, file: FileHandle) -> Result<usize, Err> {
    for fd in 0..NOFILE {
        if process.file(fd).is_none() {
            let previous = process.replace_file(fd, Some(file));
            debug_assert!(previous.is_none());
            drop(previous);
            return Ok(fd);
        }
    }
    Err(Err::BadArg)
}

fn fetch_str(process: &CurrentProc, addr: u64, dst: &mut [u8]) -> Result<usize, Err> {
    uvm::copy_instr(process.pagetable(), dst, addr)
}

fn fetch_addr(process: &CurrentProc, addr: u64) -> Result<u64, Err> {
    let mut bytes = [0; size_of::<u64>()];
    uvm::copy_in(process.pagetable(), &mut bytes, addr)?;
    Ok(u64::from_le_bytes(bytes))
}

pub fn dup(process: &CurrentProc) -> Result<usize, Err> {
    let (_, file) = arg_fd(process, 0)?;
    fd_alloc(process, file)
}

pub fn read(process: &CurrentProc) -> Result<usize, Err> {
    let (_, file) = arg_fd(process, 0)?;
    let dst = arg_addr(process, 1);
    let n = arg_int(process, 2);
    let n = usize::try_from(n).map_err(|_| Err::BadArg)?;
    file.read(true, dst, n)
}

pub fn write(process: &CurrentProc) -> Result<usize, Err> {
    let (_, file) = arg_fd(process, 0)?;
    let src = arg_addr(process, 1);
    let n = arg_int(process, 2);
    let n = usize::try_from(n).map_err(|_| Err::BadArg)?;
    file.write(true, src, n)
}

pub fn close(process: &CurrentProc) -> Result<usize, Err> {
    let (fd, file) = arg_fd(process, 0)?;
    let stored = process.replace_file(fd, None).ok_or(Err::BadArg)?;
    drop(file);
    drop(stored);
    Ok(0)
}

pub fn fstat(process: &CurrentProc) -> Result<usize, Err> {
    let (_, file) = arg_fd(process, 0)?;
    let stat = file.stat()?;
    let bytes = encode_stat(stat);
    uvm::copy_out(process.pagetable(), arg_addr(process, 1), &bytes)?;
    Ok(0)
}

pub fn open(process: &CurrentProc) -> Result<usize, Err> {
    let mut path = [0; MAXPATH];
    let n = fetch_str(process, arg_addr(process, 0), &mut path)?;
    let path = &path[..n - 1];
    let mode = arg_int(process, 1);
    let operation = log::begin_op();
    let inode = if mode & O_CREATE != 0 {
        create(path, FileType::File, 0, 0).ok_or(Err::BadArg)?
    } else {
        inode::namei(path).ok_or(Err::NoEnt)?
    };

    let (kind, major) = {
        let mut guard = inode.lock();
        let kind = guard.kind();
        if kind == FileType::Dir as i16 && mode != O_RDONLY {
            return Err(Err::BadArg);
        }
        let major = guard.major();
        if kind == FileType::Device as i16
            && (major < 0 || usize::try_from(major).map_or(true, |major| major >= NDEV))
        {
            return Err(Err::BadArg);
        }
        if mode & O_TRUNC != 0 && kind == FileType::File as i16 {
            guard.truncate();
        }
        (kind, major)
    };

    let readable = mode & O_WRONLY == 0;
    let writable = mode & (O_WRONLY | O_RDWR) != 0;
    let file = if kind == FileType::Device as i16 {
        drop(inode);
        FileHandle::device(major, readable, writable).ok_or(Err::BadArg)?
    } else {
        FileHandle::inode(inode, readable, writable).ok_or(Err::BadArg)?
    };
    let fd = fd_alloc(process, file)?;
    drop(operation);
    Ok(fd)
}

pub fn mknod(process: &CurrentProc) -> Result<usize, Err> {
    let mut path = [0; MAXPATH];
    let n = fetch_str(process, arg_addr(process, 0), &mut path)?;
    let major = arg_int(process, 1);
    let minor = arg_int(process, 2);
    let major = i16::try_from(major).map_err(|_| Err::BadArg)?;
    let minor = i16::try_from(minor).map_err(|_| Err::BadArg)?;
    let operation = log::begin_op();
    let inode = create(&path[..n - 1], FileType::Device, major, minor).ok_or(Err::BadArg)?;
    drop(inode);
    drop(operation);
    Ok(0)
}

pub fn chdir(process: &CurrentProc) -> Result<usize, Err> {
    let mut path = [0; MAXPATH];
    let n = fetch_str(process, arg_addr(process, 0), &mut path)?;
    let operation = log::begin_op();
    let inode = inode::namei(&path[..n - 1]).ok_or(Err::NoEnt)?;
    if inode.lock().kind() != FileType::Dir as i16 {
        return Err(Err::BadArg);
    }
    let old = process.cwd();
    process.set_cwd(Some(inode));
    drop(old);
    drop(operation);
    Ok(0)
}

pub fn exec(process: &CurrentProc) -> Result<usize, Err> {
    let mut path = [0; MAXPATH];
    let path_len = fetch_str(process, arg_addr(process, 0), &mut path)?;
    let uargv = arg_addr(process, 1);
    let mut arena = [0u8; PAGE_SIZE];
    let mut ranges = [(0usize, 0usize); MAXARG];
    let mut used = 0;
    let mut argc = None;
    for (index, range) in ranges.iter_mut().enumerate() {
        let offset = u64::try_from(index * size_of::<u64>()).map_err(|_| Err::BadArg)?;
        let user_arg = fetch_addr(process, uargv.checked_add(offset).ok_or(Err::BadArg)?)?;
        if user_arg == 0 {
            argc = Some(index);
            break;
        }
        let len = fetch_str(process, user_arg, &mut arena[used..])?;
        *range = (used, used + len - 1);
        used += len;
    }
    let argc = argc.ok_or(Err::BadArg)?;
    let args: [&[u8]; MAXARG] = core::array::from_fn(|index| {
        let (start, end) = ranges[index];
        &arena[start..end]
    });
    exec::exec(&path[..path_len - 1], &args[..argc])
}

fn create(path: &[u8], kind: FileType, major: i16, minor: i16) -> Option<Inode> {
    let (parent, name) = inode::nameiparent(path)?;
    let mut parent_guard = parent.lock();
    if parent_guard.nlink() == 0 {
        return None;
    }
    if let Some(existing) = parent_guard.dir_lookup(&name, None) {
        drop(parent_guard);
        drop(parent);
        let existing_kind = existing.lock().kind();
        return (kind == FileType::File
            && (existing_kind == FileType::File as i16
                || existing_kind == FileType::Device as i16))
            .then_some(existing);
    }

    let created = inode::alloc(parent.dev(), kind)?;
    let mut created_guard = created.lock();
    created_guard.set_device(major, minor);
    created_guard.set_nlink(1);
    created_guard.update();
    if !parent_guard.dir_link(&name, created.inum()) {
        // Keep the ordinary ownership path on failure: mark the inode
        // unlinked, release both sleep locks, then let `drop(created)` run
        // iput and reclaim it. No leaked reference or forgotten guard.
        created_guard.set_nlink(0);
        created_guard.update();
        drop(created_guard);
        drop(parent_guard);
        drop(created);
        drop(parent);
        return None;
    }
    drop(created_guard);
    drop(parent_guard);
    drop(parent);
    Some(created)
}

fn encode_stat(stat: Stat) -> [u8; size_of::<Stat>()] {
    let mut bytes = [0; size_of::<Stat>()];
    bytes[0..4].copy_from_slice(&stat.dev.to_le_bytes());
    bytes[4..8].copy_from_slice(&stat.ino.to_le_bytes());
    bytes[8..10].copy_from_slice(&stat.r#type.to_le_bytes());
    bytes[10..12].copy_from_slice(&stat.nlink.to_le_bytes());
    bytes[16..24].copy_from_slice(&stat.size.to_le_bytes());
    bytes
}
