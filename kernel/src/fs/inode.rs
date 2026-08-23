//! Inode cache, block mapping, directories, and path traversal (`fs.c`).

use core::cell::UnsafeCell;

use abi::{
    BPB, BSIZE, DIRSIZ, Dinode, Dirent, FileType, IPB, MAXFILE, NDIRECT, NINDIRECT, ROOTDEV,
    ROOTINO, Stat, bitmap_block, inode_block,
};

use super::{bcache, log, superblock};
use crate::params::NINODE;
use crate::proc;
use crate::sync::{SleepLock, SpinLock};

#[derive(Clone, Copy)]
struct TableEntry {
    dev: u32,
    inum: u32,
    refs: u32,
    valid: bool,
    reclaiming: bool,
}

impl TableEntry {
    const EMPTY: Self = Self {
        dev: 0,
        inum: 0,
        refs: 0,
        valid: false,
        reclaiming: false,
    };
}

struct InodeSlot {
    lock: SleepLock,
    metadata: UnsafeCell<Dinode>,
}

// SAFETY: metadata is accessed only while this slot's SleepLock is held.
unsafe impl Sync for InodeSlot {}

static SLOTS: [InodeSlot; NINODE] = [const {
    InodeSlot {
        lock: SleepLock::new(),
        metadata: UnsafeCell::new(Dinode {
            r#type: 0,
            major: 0,
            minor: 0,
            nlink: 0,
            size: 0,
            addrs: [0; NDIRECT + 1],
        }),
    }
}; NINODE];
static ITABLE: SpinLock<[TableEntry; NINODE]> = SpinLock::new([TableEntry::EMPTY; NINODE]);

/// A counted reference to one in-memory inode-table slot.
pub struct Inode {
    index: usize,
}

impl Inode {
    pub fn lock(&self) -> InodeGuard<'_> {
        let (dev, inum, refs) = {
            let table = ITABLE.lock();
            let entry = table[self.index];
            (entry.dev, entry.inum, entry.refs)
        };
        assert!(refs != 0, "ilock");
        SLOTS[self.index].lock.acquire();

        let valid = ITABLE.lock()[self.index].valid;
        if !valid {
            let sb = superblock();
            let block = bcache::bread(dev, inode_block(inum, sb.inodestart));
            let at = inum as usize % IPB as usize * Dinode::ENCODED_LEN;
            let inode = Dinode::decode(&block.data()[at..at + Dinode::ENCODED_LEN])
                .expect("invalid dinode encoding");
            if inode.r#type == 0 {
                SLOTS[self.index].lock.release();
                panic!("ilock: no type");
            }
            // SAFETY: this flow holds the slot's SleepLock exclusively.
            unsafe { *SLOTS[self.index].metadata.get() = inode };
            // `valid` is an itable-owned state transition. iget never takes
            // the SleepLock while holding ITABLE.
            ITABLE.lock()[self.index].valid = true;
        }
        InodeGuard { inode: self }
    }

    pub fn dev(&self) -> u32 {
        ITABLE.lock()[self.index].dev
    }

    pub fn inum(&self) -> u32 {
        ITABLE.lock()[self.index].inum
    }
}

impl Clone for Inode {
    fn clone(&self) -> Self {
        let mut table = ITABLE.lock();
        assert!(table[self.index].refs != 0, "idup");
        table[self.index].refs += 1;
        Self { index: self.index }
    }
}

impl Drop for Inode {
    /// `iput`: the caller owns the surrounding transaction. This path never
    /// starts a nested operation.
    fn drop(&mut self) {
        let mut table = ITABLE.lock();
        assert!(table[self.index].refs != 0, "iput");
        if table[self.index].refs != 1 || !table[self.index].valid {
            table[self.index].refs -= 1;
            return;
        }

        // Reserve this identity, then release ITABLE before the potentially
        // blocking inode lock and disk work.
        table[self.index].reclaiming = true;
        let dev = table[self.index].dev;
        let inum = table[self.index].inum;
        drop(table);
        SLOTS[self.index].lock.acquire();

        // SAFETY: the slot's SleepLock is held.
        let metadata = unsafe { &mut *SLOTS[self.index].metadata.get() };
        let reclaimed = metadata.nlink == 0;
        if reclaimed {
            truncate(dev, inum, metadata);
            free_disk_inode(dev, inum);
        }
        SLOTS[self.index].lock.release();

        let mut table = ITABLE.lock();
        if reclaimed {
            table[self.index].valid = false;
        }
        table[self.index].reclaiming = false;
        table[self.index].refs -= 1;
        drop(table);
        proc::wakeup(ITABLE.chan());
    }
}

pub fn get(dev: u32, inum: u32) -> Inode {
    let mut table = ITABLE.lock();
    loop {
        let mut empty = None;
        let mut waiting = false;
        for (index, entry) in table.iter_mut().enumerate() {
            if entry.refs != 0 && entry.dev == dev && entry.inum == inum {
                if entry.reclaiming {
                    waiting = true;
                    break;
                }
                entry.refs += 1;
                return Inode { index };
            }
            if empty.is_none() && entry.refs == 0 {
                empty = Some(index);
            }
        }
        if waiting {
            table = proc::sleep(ITABLE.chan(), table);
            continue;
        }
        let index = empty.expect("iget: no inodes");
        table[index] = TableEntry {
            dev,
            inum,
            refs: 1,
            valid: false,
            reclaiming: false,
        };
        return Inode { index };
    }
}
/// Reclaim on-disk inodes left unlinked by a crash (`ireclaim`).
pub fn reclaim(dev: u32) {
    let sb = superblock();
    for inum in 1..sb.ninodes {
        let block = bcache::bread(dev, inode_block(inum, sb.inodestart));
        let at = inum as usize % IPB as usize * Dinode::ENCODED_LEN;
        let disk_inode = Dinode::decode(&block.data()[at..at + Dinode::ENCODED_LEN])
            .expect("invalid dinode encoding");
        let orphaned = disk_inode.r#type != 0 && disk_inode.nlink == 0;
        drop(block);
        if !orphaned {
            continue;
        }

        crate::printk::line(format_args!("ireclaim: orphaned inode {inum}"));
        let operation = log::begin_op();
        let inode = get(dev, inum);
        drop(inode.lock());
        drop(inode);
        drop(operation);
        crate::printk::line(format_args!("ireclaim: completed inode {inum}"));
    }
}

/// Allocate a free on-disk inode. Caller owns a transaction.
pub fn alloc(dev: u32, kind: FileType) -> Option<Inode> {
    let sb = superblock();
    for inum in 1..sb.ninodes {
        let mut block = bcache::bread(dev, inode_block(inum, sb.inodestart));
        let at = inum as usize % IPB as usize * Dinode::ENCODED_LEN;
        let inode = Dinode::decode(&block.data()[at..at + Dinode::ENCODED_LEN])?;
        if inode.r#type != 0 {
            continue;
        }
        let allocated = Dinode {
            r#type: kind as i16,
            ..Dinode::default()
        };
        block.data_mut()[at..at + Dinode::ENCODED_LEN].copy_from_slice(&allocated.encode());
        log::write(&block);
        return Some(get(dev, inum));
    }
    None
}

/// An inode held through its sleep lock.
pub struct InodeGuard<'a> {
    inode: &'a Inode,
}

impl InodeGuard<'_> {
    fn metadata(&self) -> &Dinode {
        // SAFETY: this guard owns the slot's SleepLock.
        unsafe { &*SLOTS[self.inode.index].metadata.get() }
    }

    fn metadata_mut(&mut self) -> &mut Dinode {
        // SAFETY: this guard exclusively owns the slot's SleepLock.
        unsafe { &mut *SLOTS[self.inode.index].metadata.get() }
    }

    pub fn kind(&self) -> i16 {
        self.metadata().r#type
    }

    pub fn major(&self) -> i16 {
        self.metadata().major
    }

    pub fn size(&self) -> u32 {
        self.metadata().size
    }

    pub fn nlink(&self) -> i16 {
        self.metadata().nlink
    }

    pub fn set_nlink(&mut self, nlink: i16) {
        self.metadata_mut().nlink = nlink;
    }

    pub fn set_device(&mut self, major: i16, minor: i16) {
        let metadata = self.metadata_mut();
        metadata.major = major;
        metadata.minor = minor;
    }

    pub fn update(&mut self) {
        let dev = self.inode.dev();
        let inum = self.inode.inum();
        update_disk_inode(dev, inum, *self.metadata());
    }

    pub fn truncate(&mut self) {
        let dev = self.inode.dev();
        let inum = self.inode.inum();
        truncate(dev, inum, self.metadata_mut());
    }

    pub fn stat(&self) -> Stat {
        let metadata = self.metadata();
        Stat {
            dev: self.inode.dev() as i32,
            ino: self.inode.inum(),
            r#type: metadata.r#type,
            nlink: metadata.nlink,
            size: u64::from(metadata.size),
        }
    }

    pub fn read_at(&mut self, dst: &mut [u8], mut off: u32) -> usize {
        let size = self.metadata().size;
        if off > size {
            return 0;
        }
        let total = dst.len().min((size - off) as usize);
        let dev = self.inode.dev();
        let mut done = 0;
        while done < total {
            let Some(addr) = block_addr(dev, self.metadata_mut(), off / BSIZE as u32, false) else {
                break;
            };
            let block = bcache::bread(dev, addr);
            let within = off as usize % BSIZE;
            let n = (total - done).min(BSIZE - within);
            dst[done..done + n].copy_from_slice(&block.data()[within..within + n]);
            done += n;
            off += n as u32;
        }
        done
    }

    pub fn write_at(&mut self, src: &[u8], mut off: u32) -> usize {
        let end = off.checked_add(src.len() as u32);
        if off > self.metadata().size
            || end.is_none()
            || end.is_some_and(|end| end as usize > MAXFILE * BSIZE)
        {
            return 0;
        }
        let dev = self.inode.dev();
        let mut done = 0;
        while done < src.len() {
            let Some(addr) = block_addr(dev, self.metadata_mut(), off / BSIZE as u32, true) else {
                break;
            };
            let mut block = bcache::bread(dev, addr);
            let within = off as usize % BSIZE;
            let n = (src.len() - done).min(BSIZE - within);
            block.data_mut()[within..within + n].copy_from_slice(&src[done..done + n]);
            log::write(&block);
            done += n;
            off += n as u32;
        }
        if off > self.metadata().size {
            self.metadata_mut().size = off;
        }
        self.update();
        done
    }

    pub fn write_user_at(&mut self, src: u64, mut off: u32, n: usize) -> usize {
        let end = off.checked_add(n as u32);
        if off > self.metadata().size
            || end.is_none()
            || end.is_some_and(|end| end as usize > MAXFILE * BSIZE)
        {
            return 0;
        }
        let dev = self.inode.dev();
        let mut done = 0;
        while done < n {
            let Some(addr) = block_addr(dev, self.metadata_mut(), off / BSIZE as u32, true) else {
                break;
            };
            let mut block = bcache::bread(dev, addr);
            let within = off as usize % BSIZE;
            let amount = (n - done).min(BSIZE - within);
            let copied = proc::either_copy_in(
                &mut block.data_mut()[within..within + amount],
                true,
                src + done as u64,
            )
            .is_ok();
            // copyin may have changed the beginning of the block before
            // reaching an invalid user page.
            log::write(&block);
            if !copied {
                break;
            }
            done += amount;
            off += amount as u32;
        }
        if off > self.metadata().size {
            self.metadata_mut().size = off;
        }
        self.update();
        done
    }

    pub fn dir_lookup(&mut self, name: &[u8], offset: Option<&mut u32>) -> Option<Inode> {
        assert_eq!(self.kind(), FileType::Dir as i16, "dirlookup not DIR");
        let wanted = name_bytes(name);
        let mut at = 0;
        let mut offset = offset;
        while at < self.size() {
            let mut bytes = [0; Dirent::ENCODED_LEN];
            if self.read_at(&mut bytes, at) != bytes.len() {
                panic!("dirlookup read");
            }
            let entry = Dirent::decode(&bytes).expect("dirent encoding");
            if entry.inum != 0 && entry.name == wanted {
                if let Some(found) = offset.as_deref_mut() {
                    *found = at;
                }
                return Some(get(self.inode.dev(), u32::from(entry.inum)));
            }
            at += Dirent::ENCODED_LEN as u32;
        }
        None
    }

    pub fn dir_link(&mut self, name: &[u8], inum: u32) -> bool {
        if self.dir_lookup(name, None).is_some() {
            return false;
        }
        let mut at = 0;
        while at < self.size() {
            let mut bytes = [0; Dirent::ENCODED_LEN];
            if self.read_at(&mut bytes, at) != bytes.len() {
                panic!("dirlink read");
            }
            if Dirent::decode(&bytes).expect("dirent encoding").inum == 0 {
                break;
            }
            at += Dirent::ENCODED_LEN as u32;
        }
        let Some(entry) = Dirent::new(inum as u16, name) else {
            return false;
        };
        self.write_at(&entry.encode(), at) == Dirent::ENCODED_LEN
    }
}

impl Drop for InodeGuard<'_> {
    fn drop(&mut self) {
        SLOTS[self.inode.index].lock.release();
    }
}

fn update_disk_inode(dev: u32, inum: u32, inode: Dinode) {
    let sb = superblock();
    let mut block = bcache::bread(dev, inode_block(inum, sb.inodestart));
    let at = inum as usize % IPB as usize * Dinode::ENCODED_LEN;
    block.data_mut()[at..at + Dinode::ENCODED_LEN].copy_from_slice(&inode.encode());
    log::write(&block);
}

fn free_disk_inode(dev: u32, inum: u32) {
    let sb = superblock();
    let mut block = bcache::bread(dev, inode_block(inum, sb.inodestart));
    let at = inum as usize % IPB as usize * Dinode::ENCODED_LEN;
    let mut inode = Dinode::decode(&block.data()[at..at + Dinode::ENCODED_LEN])
        .expect("invalid dinode encoding");
    inode.r#type = 0;
    block.data_mut()[at..at + Dinode::ENCODED_LEN].copy_from_slice(&inode.encode());
    log::write(&block);
}

fn zero_block(dev: u32, blockno: u32) {
    let mut block = bcache::bread(dev, blockno);
    block.data_mut().fill(0);
    log::write(&block);
}

fn alloc_block(dev: u32) -> Option<u32> {
    let sb = superblock();
    let mut base = 0;
    while base < sb.size {
        let mut bitmap = bcache::bread(dev, bitmap_block(base, sb.bmapstart));
        let count = BPB.min(sb.size - base);
        for bit in 0..count {
            let mask = 1 << (bit % 8);
            let byte = &mut bitmap.data_mut()[(bit / 8) as usize];
            if *byte & mask != 0 {
                continue;
            }
            *byte |= mask;
            log::write(&bitmap);
            drop(bitmap);
            let blockno = base + bit;
            zero_block(dev, blockno);
            return Some(blockno);
        }
        base += BPB;
    }
    None
}

fn free_block(dev: u32, blockno: u32) {
    let sb = superblock();
    let mut bitmap = bcache::bread(dev, bitmap_block(blockno, sb.bmapstart));
    let bit = blockno % BPB;
    let mask = 1 << (bit % 8);
    let byte = &mut bitmap.data_mut()[(bit / 8) as usize];
    assert!(*byte & mask != 0, "freeing free block");
    *byte &= !mask;
    log::write(&bitmap);
}

fn block_addr(dev: u32, inode: &mut Dinode, mut bn: u32, allocate: bool) -> Option<u32> {
    if bn < NDIRECT as u32 {
        // Reassign the outer address after allocation; do not shadow it with
        // an inner `let`, or the direct path returns zero.
        let mut addr = inode.addrs[bn as usize];
        if addr == 0 && allocate {
            addr = alloc_block(dev)?;
            inode.addrs[bn as usize] = addr;
        }
        return (addr != 0).then_some(addr);
    }

    bn -= NDIRECT as u32;
    if bn >= NINDIRECT as u32 {
        panic!("bmap: out of range");
    }
    let mut indirect_addr = inode.addrs[NDIRECT];
    if indirect_addr == 0 && allocate {
        indirect_addr = alloc_block(dev)?;
        inode.addrs[NDIRECT] = indirect_addr;
    }
    if indirect_addr == 0 {
        return None;
    }
    let mut indirect = bcache::bread(dev, indirect_addr);
    let at = bn as usize * 4;
    let mut addr = u32::from_le_bytes(
        indirect.data()[at..at + 4]
            .try_into()
            .expect("indirect entry"),
    );
    if addr == 0 && allocate {
        addr = alloc_block(dev)?;
        indirect.data_mut()[at..at + 4].copy_from_slice(&addr.to_le_bytes());
        log::write(&indirect);
    }
    (addr != 0).then_some(addr)
}

fn truncate(dev: u32, inum: u32, inode: &mut Dinode) {
    for addr in &mut inode.addrs[..NDIRECT] {
        if *addr != 0 {
            free_block(dev, *addr);
            *addr = 0;
        }
    }
    let indirect_addr = inode.addrs[NDIRECT];
    if indirect_addr != 0 {
        let indirect = bcache::bread(dev, indirect_addr);
        for bytes in indirect.data().as_chunks::<4>().0 {
            let addr = u32::from_le_bytes(*bytes);
            if addr != 0 {
                free_block(dev, addr);
            }
        }
        drop(indirect);
        free_block(dev, indirect_addr);
        inode.addrs[NDIRECT] = 0;
    }
    inode.size = 0;
    update_disk_inode(dev, inum, *inode);
}

fn name_bytes(name: &[u8]) -> [u8; DIRSIZ] {
    let mut result = [0; DIRSIZ];
    let n = name.len().min(DIRSIZ);
    result[..n].copy_from_slice(&name[..n]);
    result
}

pub fn namei(path: &[u8]) -> Option<Inode> {
    namex(path, false).map(|(inode, _)| inode)
}

pub fn nameiparent(path: &[u8]) -> Option<(Inode, [u8; DIRSIZ])> {
    namex(path, true)
}

fn namex(mut path: &[u8], parent: bool) -> Option<(Inode, [u8; DIRSIZ])> {
    let mut inode = if path.first() == Some(&b'/') {
        get(ROOTDEV, ROOTINO)
    } else {
        proc::cwd().unwrap_or_else(|| get(ROOTDEV, ROOTINO))
    };
    let mut name = [0; DIRSIZ];
    loop {
        while path.first() == Some(&b'/') {
            path = &path[1..];
        }
        if path.is_empty() {
            return (!parent).then_some((inode, name));
        }
        let end = path
            .iter()
            .position(|byte| *byte == b'/')
            .unwrap_or(path.len());
        name = name_bytes(&path[..end]);
        path = &path[end..];
        while path.first() == Some(&b'/') {
            path = &path[1..];
        }
        let mut guard = inode.lock();
        if guard.kind() != FileType::Dir as i16 || guard.nlink() == 0 {
            return None;
        }
        if parent && path.is_empty() {
            drop(guard);
            return Some((inode, name));
        }
        let next = guard.dir_lookup(&name, None)?;
        drop(guard);
        inode = next;
    }
}
