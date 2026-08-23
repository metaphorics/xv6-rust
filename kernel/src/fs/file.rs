//! System-wide open-file table (`kernel/file.c`).

use core::sync::atomic::{AtomicU32, Ordering};

use abi::{BSIZE, FileType, MAXOPBLOCKS, Stat};

use super::inode::Inode;
use super::log;
use crate::dev::console;
use crate::err::Err;
use crate::params::NFILE;
use crate::pipe::{self, PipeEnd};
use crate::proc;
use crate::sync::SpinLock;

const CONSOLE: i16 = 1;
const WRITE_CHUNK: usize = ((MAXOPBLOCKS - 1 - 1 - 2) / 2) * BSIZE;

struct FileSlot {
    refs: u32,
    readable: bool,
    writable: bool,
    kind: FileKind,
}

enum FileKind {
    None,
    Device(i16),
    Pipe(PipeEnd),
    Inode(Inode),
}

static FILES: SpinLock<[FileSlot; NFILE]> = SpinLock::new(
    [const {
        FileSlot {
            refs: 0,
            readable: false,
            writable: false,
            kind: FileKind::None,
        }
    }; NFILE],
);
static OFFSETS: [AtomicU32; NFILE] = [const { AtomicU32::new(0) }; NFILE];

/// Counted reference to one system-wide file slot.
pub struct FileHandle {
    index: usize,
}

impl FileHandle {
    pub fn inode(inode: Inode, readable: bool, writable: bool) -> Option<Self> {
        let handle = alloc()?;
        let mut files = FILES.lock();
        files[handle.index].readable = readable;
        files[handle.index].writable = writable;
        files[handle.index].kind = FileKind::Inode(inode);
        drop(files);
        Some(handle)
    }

    pub fn device(major: i16, readable: bool, writable: bool) -> Option<Self> {
        let handle = alloc()?;
        let mut files = FILES.lock();
        files[handle.index].readable = readable;
        files[handle.index].writable = writable;
        files[handle.index].kind = FileKind::Device(major);
        drop(files);
        Some(handle)
    }

    pub fn pipe_pair() -> Option<(Self, Self)> {
        let (read_end, write_end) = pipe::alloc()?;
        let Some(read) = alloc() else {
            read_end.close();
            write_end.close();
            return None;
        };
        {
            let mut files = FILES.lock();
            files[read.index].readable = true;
            files[read.index].kind = FileKind::Pipe(read_end);
        }
        let Some(write) = alloc() else {
            drop(read);
            write_end.close();
            return None;
        };
        {
            let mut files = FILES.lock();
            files[write.index].writable = true;
            files[write.index].kind = FileKind::Pipe(write_end);
        }
        Some((read, write))
    }

    pub fn read(&self, user_dst: bool, mut dst: u64, n: usize) -> Result<usize, Err> {
        let files = FILES.lock();
        let slot = &files[self.index];
        if !slot.readable {
            return Err(Err::BadArg);
        }
        match &slot.kind {
            FileKind::Device(major) => {
                let major = *major;
                drop(files);
                if major != CONSOLE {
                    return Err(Err::BadArg);
                }
                console::read(user_dst, dst, n)
            }
            FileKind::Pipe(pipe) => {
                let pipe = *pipe;
                drop(files);
                pipe.read(user_dst, dst, n)
            }
            FileKind::Inode(inode) => {
                let inode = inode.clone();
                drop(files);
                let mut inode = inode.lock();
                // Shared descriptor offset load and advance are both inside
                // the inode lock, preserving sharedfd atomicity.
                let mut off = OFFSETS[self.index].load(Ordering::Relaxed);
                let mut total = 0;
                let mut buffer = [0; 512];
                while total < n {
                    let wanted = (n - total).min(buffer.len());
                    let read = inode.read_at(&mut buffer[..wanted], off);
                    if read == 0 {
                        break;
                    }
                    proc::either_copy_out(&buffer[..read], user_dst, dst)?;
                    off += read as u32;
                    dst += read as u64;
                    total += read;
                }
                OFFSETS[self.index].store(off, Ordering::Relaxed);
                Ok(total)
            }
            FileKind::None => Err(Err::BadArg),
        }
    }

    pub fn write(&self, user_src: bool, mut src: u64, n: usize) -> Result<usize, Err> {
        let files = FILES.lock();
        let slot = &files[self.index];
        if !slot.writable {
            return Err(Err::BadArg);
        }
        match &slot.kind {
            FileKind::Device(major) => {
                let major = *major;
                drop(files);
                if major != CONSOLE {
                    return Err(Err::BadArg);
                }
                Ok(console::write(user_src, src, n))
            }
            FileKind::Pipe(pipe) => {
                let pipe = *pipe;
                drop(files);
                pipe.write(user_src, src, n)
            }
            FileKind::Inode(inode) => {
                let inode = inode.clone();
                drop(files);
                let mut total = 0;
                while total < n {
                    let requested = (n - total).min(WRITE_CHUNK);
                    let operation = log::begin_op();
                    let mut guard = inode.lock();
                    // Keep the shared offset under the same inode lock as the
                    // write, matching file.c's sharedfd serialization.
                    let off = OFFSETS[self.index].load(Ordering::Relaxed);
                    let written = if user_src {
                        guard.write_user_at(src, off, requested)
                    } else {
                        let mut written = 0;
                        let mut buffer = [0; 512];
                        while written < requested {
                            let amount = (requested - written).min(buffer.len());
                            proc::either_copy_in(
                                &mut buffer[..amount],
                                false,
                                src + written as u64,
                            )?;
                            let accepted = guard.write_at(&buffer[..amount], off + written as u32);
                            written += accepted;
                            if accepted != amount {
                                break;
                            }
                        }
                        written
                    };
                    if written > 0 {
                        OFFSETS[self.index].store(off + written as u32, Ordering::Relaxed);
                    }
                    drop(guard);
                    drop(operation);
                    if written != requested {
                        return Err(Err::BadArg);
                    }
                    total += written;
                    src += written as u64;
                }
                Ok(total)
            }
            FileKind::None => Err(Err::BadArg),
        }
    }

    pub fn stat(&self) -> Result<Stat, Err> {
        let files = FILES.lock();
        match &files[self.index].kind {
            FileKind::Inode(inode) => {
                let inode = inode.clone();
                drop(files);
                Ok(inode.lock().stat())
            }
            FileKind::Device(major) => Ok(Stat {
                dev: 0,
                ino: 0,
                r#type: FileType::Device as i16,
                nlink: 1,
                size: *major as u64,
            }),
            FileKind::Pipe(_) => Err(Err::BadArg),
            FileKind::None => Err(Err::BadArg),
        }
    }
}

fn alloc() -> Option<FileHandle> {
    let mut files = FILES.lock();
    let index = files.iter().position(|file| file.refs == 0)?;
    files[index].refs = 1;
    files[index].readable = false;
    files[index].writable = false;
    files[index].kind = FileKind::None;
    OFFSETS[index].store(0, Ordering::Relaxed);
    Some(FileHandle { index })
}

impl Clone for FileHandle {
    fn clone(&self) -> Self {
        let mut files = FILES.lock();
        assert!(files[self.index].refs != 0, "filedup");
        files[self.index].refs += 1;
        Self { index: self.index }
    }
}

impl Drop for FileHandle {
    fn drop(&mut self) {
        let mut files = FILES.lock();
        assert!(files[self.index].refs != 0, "fileclose");
        files[self.index].refs -= 1;
        if files[self.index].refs != 0 {
            return;
        }
        let kind = core::mem::replace(&mut files[self.index].kind, FileKind::None);
        files[self.index].readable = false;
        files[self.index].writable = false;
        drop(files);

        match kind {
            FileKind::Inode(inode) => {
                // The final inode reference may reclaim blocks, so fileclose owns
                // exactly one surrounding transaction as in file.c.
                let operation = log::begin_op();
                drop(inode);
                drop(operation);
            }
            FileKind::Pipe(pipe) => pipe.close(),
            FileKind::None | FileKind::Device(_) => {}
        }
    }
}
