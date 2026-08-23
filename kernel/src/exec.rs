//! ELF program loading and argument-stack construction (`kernel/exec.c`).

use crate::arch::{MAXVA, PAGE_SIZE, PageTable, Perm};
use crate::err::Err;
use crate::fs::inode::InodeGuard;
use crate::fs::log;
use crate::mm::addr::page_round_up;
use crate::mm::uvm;
use crate::params::{MAXARG, USERSTACK};
use crate::proc;

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELF_PROG_LOAD: u32 = 1;
const ELF_FLAG_EXEC: u32 = 1;
const ELF_FLAG_WRITE: u32 = 2;
const PAGE: u64 = PAGE_SIZE as u64;

struct Image {
    table: Option<PageTable>,
    sz: u64,
}

impl Image {
    fn new(table: PageTable) -> Self {
        Self {
            table: Some(table),
            sz: 0,
        }
    }

    fn table(&self) -> &PageTable {
        self.table.as_ref().expect("exec image table")
    }

    fn table_mut(&mut self) -> &mut PageTable {
        self.table.as_mut().expect("exec image table")
    }

    fn take(&mut self) -> PageTable {
        self.table.take().expect("exec image table")
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        if let Some(table) = self.table.take() {
            // `sz` is updated after every successful allocation, including
            // the guard and stack pages. Failure therefore removes every
            // leaf before PageTable's freewalk runs.
            uvm::free_proc_table(table, self.sz);
        }
    }
}

#[derive(Clone, Copy)]
struct ProgramHeader {
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
}

/// Replace the current process with the ELF at `path` and marshal `args`
/// onto its new user stack. Argument byte slices do not include nul bytes.
pub fn exec(path: &[u8], args: &[&[u8]]) -> Result<usize, Err> {
    if args.len() > MAXARG {
        return Err(Err::BadArg);
    }
    let process = proc::my_proc().expect("exec: no current proc");
    let operation = log::begin_op();
    let inode = crate::fs::inode::namei(path).ok_or(Err::NoEnt)?;
    let mut guard = inode.lock();

    let mut header = [0; ELF_HEADER_SIZE];
    if guard.read_at(&mut header, 0) != header.len() || &header[..4] != ELF_MAGIC {
        return Err(Err::BadArg);
    }
    let entry = u64_at(&header, 24);
    let phoff = u64_at(&header, 32);
    let phentsize = u16_at(&header, 54) as usize;
    let phnum = u16_at(&header, 56) as usize;
    if phentsize != PROGRAM_HEADER_SIZE {
        return Err(Err::BadArg);
    }

    let table = process.new_exec_pagetable().ok_or(Err::NoMem)?;
    let mut image = Image::new(table);
    for index in 0..phnum {
        let offset = phoff
            .checked_add((index * PROGRAM_HEADER_SIZE) as u64)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or(Err::BadArg)?;
        let mut bytes = [0; PROGRAM_HEADER_SIZE];
        if guard.read_at(&mut bytes, offset) != bytes.len() {
            return Err(Err::BadArg);
        }
        if u32_at(&bytes, 0) != ELF_PROG_LOAD {
            continue;
        }
        let ph = ProgramHeader {
            flags: u32_at(&bytes, 4),
            offset: u64_at(&bytes, 8),
            vaddr: u64_at(&bytes, 16),
            filesz: u64_at(&bytes, 32),
            memsz: u64_at(&bytes, 40),
        };
        load_program_header(&mut image, &mut guard, ph)?;
    }
    drop(guard);
    drop(inode);
    drop(operation);

    image.sz = page_round_up(image.sz);
    let stack_top = image
        .sz
        .checked_add(((USERSTACK + 1) * PAGE_SIZE) as u64)
        .ok_or(Err::TooBig)?;
    let old_sz = image.sz;
    image.sz = uvm::alloc(image.table_mut(), old_sz, stack_top, Perm::W)?;
    let guard_page = image.sz - ((USERSTACK + 1) as u64 * PAGE);
    uvm::clear(image.table_mut(), guard_page);

    let mut sp = image.sz;
    let stack_base = sp - USERSTACK as u64 * PAGE;
    let mut pointers = [0u64; MAXARG + 1];
    let image_size = image.sz;
    for (index, arg) in args.iter().enumerate() {
        sp = sp.checked_sub((arg.len() + 1) as u64).ok_or(Err::BadArg)? & !15;
        if sp < stack_base {
            return Err(Err::BadArg);
        }
        uvm::copy_out(image.table_mut(), image_size, sp, arg)?;
        uvm::copy_out(image.table_mut(), image_size, sp + arg.len() as u64, &[0])?;
        pointers[index] = sp;
    }

    let pointer_bytes = (args.len() + 1) * size_of::<u64>();
    sp = sp.checked_sub(pointer_bytes as u64).ok_or(Err::BadArg)? & !15;
    if sp < stack_base {
        return Err(Err::BadArg);
    }
    let mut encoded = [0u8; (MAXARG + 1) * size_of::<u64>()];
    for (slot, pointer) in pointers[..=args.len()].iter().enumerate() {
        let at = slot * size_of::<u64>();
        encoded[at..at + size_of::<u64>()].copy_from_slice(&pointer.to_le_bytes());
    }
    uvm::copy_out(image.table_mut(), image_size, sp, &encoded[..pointer_bytes])?;

    let name = process_name(path);
    let sz = image.sz;
    let table = image.take();
    process.install_exec(table, sz, entry, sp, sp, name);
    Ok(args.len())
}

fn load_program_header(
    image: &mut Image,
    inode: &mut InodeGuard<'_>,
    ph: ProgramHeader,
) -> Result<(), Err> {
    if ph.memsz < ph.filesz || !ph.vaddr.is_multiple_of(PAGE) {
        return Err(Err::BadArg);
    }
    let end = ph.vaddr.checked_add(ph.memsz).ok_or(Err::BadArg)?;
    if end >= MAXVA {
        return Err(Err::BadArg);
    }
    let mut perm = Perm::R;
    if ph.flags & ELF_FLAG_EXEC != 0 {
        perm |= Perm::X;
    }
    if ph.flags & ELF_FLAG_WRITE != 0 {
        perm |= Perm::W;
    }
    let old_sz = image.sz;
    image.sz = uvm::alloc(image.table_mut(), old_sz, end, perm)?;
    load_segment(image.table(), ph.vaddr, inode, ph.offset, ph.filesz)
}

fn load_segment(
    table: &PageTable,
    va: u64,
    inode: &mut InodeGuard<'_>,
    offset: u64,
    size: u64,
) -> Result<(), Err> {
    let mut done = 0;
    while done < size {
        let pa = uvm::walkaddr(table, va + done).expect("loadseg: mapped page");
        let n = (size - done).min(PAGE) as usize;
        let file_offset =
            u32::try_from(offset.checked_add(done).ok_or(Err::BadArg)?).map_err(|_| Err::BadArg)?;
        // SAFETY: `pa` names a freshly allocated page exclusively owned by
        // this not-yet-installed image; `n` never crosses the page.
        let dst = unsafe { core::slice::from_raw_parts_mut(pa.0 as usize as *mut u8, n) };
        if inode.read_at(dst, file_offset) != n {
            return Err(Err::BadArg);
        }
        done += n as u64;
    }
    Ok(())
}

fn process_name(path: &[u8]) -> [u8; 16] {
    let last = path
        .iter()
        .rposition(|byte| *byte == b'/')
        .map_or(path, |slash| &path[slash + 1..]);
    let mut name = [0; 16];
    let n = last.len().min(name.len() - 1);
    name[..n].copy_from_slice(&last[..n]);
    name
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().expect("ELF u16"))
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("ELF u32"))
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("ELF u64"))
}
