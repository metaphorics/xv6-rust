//! Virtio block requests: one three-descriptor chain per 1 KiB fs block.

use core::cell::UnsafeCell;
use core::sync::atomic::{Ordering, fence};

use abi::BSIZE;

use super::queue::{self, DESC_F_NEXT, DESC_F_WRITE, NUM, VirtqDesc};
use super::transport;
use crate::proc;
use crate::sync::SpinLock;

const BLK_T_IN: u32 = 0;
const BLK_T_OUT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct BlkReqHeader {
    r#type: u32,
    reserved: u32,
    sector: u64,
}

impl BlkReqHeader {
    const ZERO: Self = Self {
        r#type: 0,
        reserved: 0,
        sector: 0,
    };
}

struct RequestInfo {
    status: UnsafeCell<u8>,
    chan: usize,
    active: bool,
}

impl RequestInfo {
    const fn new() -> Self {
        Self {
            status: UnsafeCell::new(0xff),
            chan: 0,
            active: false,
        }
    }
}

struct DiskState {
    free: [bool; NUM],
    used_idx: u16,
    info: [RequestInfo; NUM],
    ops: [BlkReqHeader; NUM],
}

impl DiskState {
    const fn new() -> Self {
        Self {
            free: [true; NUM],
            used_idx: 0,
            info: [const { RequestInfo::new() }; NUM],
            ops: [BlkReqHeader::ZERO; NUM],
        }
    }

    fn alloc_desc(&mut self) -> Option<usize> {
        let index = self.free.iter().position(|free| *free)?;
        self.free[index] = false;
        Some(index)
    }

    fn alloc_three(&mut self) -> Option<[usize; 3]> {
        let mut result = [0; 3];
        for slot in 0..result.len() {
            let Some(index) = self.alloc_desc() else {
                for allocated in &result[..slot] {
                    self.free_desc(*allocated);
                }
                return None;
            };
            result[slot] = index;
        }
        Some(result)
    }

    fn free_desc(&mut self, index: usize) {
        assert!(index < NUM && !self.free[index], "virtio free descriptor");
        queue::write_desc(index, VirtqDesc::ZERO);
        self.free[index] = true;
    }

    fn free_chain(&mut self, mut index: usize) {
        loop {
            let desc = queue::read_desc(index);
            self.free_desc(index);
            if desc.flags & DESC_F_NEXT == 0 {
                break;
            }
            index = usize::from(desc.next);
        }
    }
}

static DISK: SpinLock<DiskState> = SpinLock::new(DiskState::new());

pub fn init() {
    let (desc, avail, used) = queue::addresses();
    let mut disk = DISK.lock();
    *disk = DiskState::new();
    transport::init(desc, avail, used, NUM as u32);
}

/// Transfer one fs block. `chan` uniquely identifies the cache buffer and is
/// the sleep/wakeup rendezvous used by the completion interrupt.
pub fn rw(blockno: u32, data: &mut [u8; BSIZE], write: bool, chan: usize) {
    let mut disk = DISK.lock();
    let indexes = loop {
        if let Some(indexes) = disk.alloc_three() {
            break indexes;
        }
        disk = proc::sleep(DISK.chan(), disk);
    };
    let [head, data_desc, status_desc] = indexes;

    disk.ops[head] = BlkReqHeader {
        r#type: if write { BLK_T_OUT } else { BLK_T_IN },
        reserved: 0,
        sector: u64::from(blockno) * (BSIZE as u64 / 512),
    };
    let header_addr = core::ptr::addr_of!(disk.ops[head]) as u64;
    queue::write_desc(
        head,
        VirtqDesc {
            addr: header_addr,
            len: core::mem::size_of::<BlkReqHeader>() as u32,
            flags: DESC_F_NEXT,
            next: data_desc as u16,
        },
    );
    queue::write_desc(
        data_desc,
        VirtqDesc {
            addr: data.as_mut_ptr() as u64,
            len: BSIZE as u32,
            flags: DESC_F_NEXT | if write { 0 } else { DESC_F_WRITE },
            next: status_desc as u16,
        },
    );

    disk.info[head].chan = chan;
    disk.info[head].active = true;
    // SAFETY: caller holds the disk lock and this request slot is exclusively
    // driver-owned until it is published to the device below.
    unsafe { core::ptr::write_volatile(disk.info[head].status.get(), 0xff) };
    queue::write_desc(
        status_desc,
        VirtqDesc {
            addr: disk.info[head].status.get() as u64,
            len: 1,
            flags: DESC_F_WRITE,
            next: 0,
        },
    );

    queue::push_avail(head as u16);
    fence(Ordering::SeqCst);
    transport::notify();

    while disk.info[head].active {
        disk = proc::sleep(chan, disk);
    }
    // SAFETY: used.idx published completion and the interrupt fenced before
    // clearing active, so the device has finished writing this status byte.
    let status = unsafe { core::ptr::read_volatile(disk.info[head].status.get()) };
    if status != 0 {
        panic!("virtio disk status");
    }
    disk.info[head].chan = 0;
    disk.free_chain(head);
    drop(disk);
    proc::wakeup(DISK.chan());
}

/// Complete every used-ring entry the device has published.
pub fn intr() {
    let mut disk = DISK.lock();
    transport::acknowledge_interrupt();
    fence(Ordering::SeqCst);

    while disk.used_idx != queue::used_idx() {
        fence(Ordering::SeqCst);
        let id = queue::used_id(disk.used_idx) as usize;
        if id >= NUM || !disk.info[id].active {
            panic!("virtio disk completion");
        }
        // SAFETY: the device published the used entry after writing status.
        let status = unsafe { core::ptr::read_volatile(disk.info[id].status.get()) };
        if status != 0 {
            panic!("virtio disk interrupt status");
        }
        disk.info[id].active = false;
        let chan = disk.info[id].chan;
        disk.used_idx = disk.used_idx.wrapping_add(1);
        proc::wakeup(chan);
    }
}
