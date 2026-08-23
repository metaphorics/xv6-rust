//! LRU buffer cache (`kernel/bio.c`) with index-based links.

use core::cell::UnsafeCell;

use abi::BSIZE;

use crate::dev::virtio::blk;
use crate::params::NBUF;
use crate::sync::{SleepLock, SpinLock};

struct Buffer {
    lock: SleepLock,
    data: UnsafeCell<[u8; BSIZE]>,
}

// SAFETY: data is accessible only while the buffer's SleepLock is held.
unsafe impl Sync for Buffer {}

static BUFS: [Buffer; NBUF] = [const {
    Buffer {
        lock: SleepLock::new(),
        data: UnsafeCell::new([0; BSIZE]),
    }
}; NBUF];

#[derive(Clone, Copy)]
struct Meta {
    dev: u32,
    blockno: u32,
    valid: bool,
    refs: u32,
    prev: Option<usize>,
    next: Option<usize>,
}

impl Meta {
    const EMPTY: Self = Self {
        dev: 0,
        blockno: 0,
        valid: false,
        refs: 0,
        prev: None,
        next: None,
    };
}

struct CacheState {
    meta: [Meta; NBUF],
    head: Option<usize>,
    tail: Option<usize>,
}

impl CacheState {
    const fn new() -> Self {
        let mut meta = [Meta::EMPTY; NBUF];
        let mut index = 0;
        while index < NBUF {
            meta[index].prev = if index == 0 { None } else { Some(index - 1) };
            meta[index].next = if index + 1 == NBUF {
                None
            } else {
                Some(index + 1)
            };
            index += 1;
        }
        Self {
            meta,
            head: if NBUF == 0 { None } else { Some(0) },
            tail: if NBUF == 0 { None } else { Some(NBUF - 1) },
        }
    }

    fn move_to_head(&mut self, index: usize) {
        if self.head == Some(index) {
            return;
        }
        let prev = self.meta[index].prev;
        let next = self.meta[index].next;
        if let Some(prev) = prev {
            self.meta[prev].next = next;
        }
        if let Some(next) = next {
            self.meta[next].prev = prev;
        } else {
            self.tail = prev;
        }
        self.meta[index].prev = None;
        self.meta[index].next = self.head;
        if let Some(head) = self.head {
            self.meta[head].prev = Some(index);
        } else {
            self.tail = Some(index);
        }
        self.head = Some(index);
    }
}

static CACHE: SpinLock<CacheState> = SpinLock::new(CacheState::new());

/// Return a locked buffer containing `dev:blockno` (`bread`, bio.c:85-94).
pub fn bread(dev: u32, blockno: u32) -> BufGuard {
    let mut buffer = bget(dev, blockno);
    if !buffer.valid() {
        let chan = buffer.chan();
        blk::rw(blockno, buffer.data_mut(), false, chan);
        CACHE.lock().meta[buffer.index].valid = true;
    }
    buffer
}

fn bget(dev: u32, blockno: u32) -> BufGuard {
    let mut cache = CACHE.lock();
    if let Some(index) = cache
        .meta
        .iter()
        .position(|meta| meta.dev == dev && meta.blockno == blockno)
    {
        // A cached identity remains a hit at refcount zero; brelse only moves
        // it in the LRU list, it does not invalidate the identity.
        cache.meta[index].refs += 1;
        drop(cache);
        BUFS[index].lock.acquire();
        return BufGuard { index };
    }

    let mut candidate = cache.tail;
    while let Some(index) = candidate {
        if cache.meta[index].refs == 0 {
            cache.meta[index].dev = dev;
            cache.meta[index].blockno = blockno;
            cache.meta[index].valid = false;
            cache.meta[index].refs = 1;
            drop(cache);
            BUFS[index].lock.acquire();
            return BufGuard { index };
        }
        candidate = cache.meta[index].prev;
    }
    panic!("bget: no buffers");
}

/// A cache buffer held through its per-buffer sleep lock.
pub struct BufGuard {
    index: usize,
}

impl BufGuard {
    pub fn blockno(&self) -> u32 {
        CACHE.lock().meta[self.index].blockno
    }

    pub fn data(&self) -> &[u8; BSIZE] {
        // SAFETY: this guard owns the buffer's SleepLock.
        unsafe { &*BUFS[self.index].data.get() }
    }

    pub fn data_mut(&mut self) -> &mut [u8; BSIZE] {
        // SAFETY: this guard exclusively owns the buffer's SleepLock.
        unsafe { &mut *BUFS[self.index].data.get() }
    }

    fn valid(&self) -> bool {
        CACHE.lock().meta[self.index].valid
    }

    fn chan(&self) -> usize {
        &BUFS[self.index] as *const Buffer as usize
    }

    /// Write this buffer to disk (`bwrite`, bio.c:97-105).
    pub fn write(&mut self) {
        let blockno = self.blockno();
        let chan = self.chan();
        blk::rw(blockno, self.data_mut(), true, chan);
    }
}

impl Drop for BufGuard {
    fn drop(&mut self) {
        BUFS[self.index].lock.release();
        let mut cache = CACHE.lock();
        let meta = &mut cache.meta[self.index];
        assert!(meta.refs != 0, "brelse");
        meta.refs -= 1;
        if meta.refs == 0 {
            cache.move_to_head(self.index);
        }
    }
}

/// Keep a logged destination resident until commit (`bpin`, bio.c:129-134).
pub fn pin(buffer: &BufGuard) {
    CACHE.lock().meta[buffer.index].refs += 1;
}

/// Release the log's pin after installation (`bunpin`, bio.c:137-142).
pub fn unpin(buffer: &BufGuard) {
    let mut cache = CACHE.lock();
    let meta = &mut cache.meta[buffer.index];
    assert!(meta.refs > 1, "bunpin");
    meta.refs -= 1;
}
