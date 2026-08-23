//! Virtqueue memory shared with the block device (virtio 1.1 section 2.6).

use core::cell::UnsafeCell;

pub const NUM: usize = 8;
pub const DESC_F_NEXT: u16 = 1;
pub const DESC_F_WRITE: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

impl VirtqDesc {
    pub const ZERO: Self = Self {
        addr: 0,
        len: 0,
        flags: 0,
        next: 0,
    };
}

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; NUM],
    unused: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; NUM],
}

#[repr(C, align(4096))]
struct DescPage {
    desc: [VirtqDesc; NUM],
}

#[repr(C, align(4096))]
struct AvailPage {
    avail: VirtqAvail,
}

#[repr(C, align(4096))]
struct UsedPage {
    used: VirtqUsed,
}

struct Shared<T>(UnsafeCell<T>);

// SAFETY: these pages are the virtqueue's DMA memory. The driver serializes
// CPU access with the disk lock; the device accesses them only according to
// the ownership transitions and fences in blk.rs.
unsafe impl<T> Sync for Shared<T> {}

static DESC: Shared<DescPage> = Shared(UnsafeCell::new(DescPage {
    desc: [VirtqDesc::ZERO; NUM],
}));
static AVAIL: Shared<AvailPage> = Shared(UnsafeCell::new(AvailPage {
    avail: VirtqAvail {
        flags: 0,
        idx: 0,
        ring: [0; NUM],
        unused: 0,
    },
}));
static USED: Shared<UsedPage> = Shared(UnsafeCell::new(UsedPage {
    used: VirtqUsed {
        flags: 0,
        idx: 0,
        ring: [VirtqUsedElem { id: 0, len: 0 }; NUM],
    },
}));

pub fn addresses() -> (u64, u64, u64) {
    // SAFETY: only addresses are formed; no reference to DMA-mutated memory
    // escapes. Static queue pages live for the device's whole lifetime.
    unsafe {
        (
            core::ptr::addr_of_mut!((*DESC.0.get()).desc) as u64,
            core::ptr::addr_of_mut!((*AVAIL.0.get()).avail) as u64,
            core::ptr::addr_of_mut!((*USED.0.get()).used) as u64,
        )
    }
}

pub fn write_desc(index: usize, value: VirtqDesc) {
    assert!(index < NUM, "virtio descriptor index");
    // SAFETY: caller holds the disk lock and owns this free descriptor.
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!((*DESC.0.get()).desc[index]), value);
    }
}

pub fn read_desc(index: usize) -> VirtqDesc {
    assert!(index < NUM, "virtio descriptor index");
    // SAFETY: caller holds the disk lock; descriptors are driver-owned.
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*DESC.0.get()).desc[index])) }
}

pub fn push_avail(head: u16) {
    // SAFETY: caller holds the disk lock. The ring entry is published before
    // idx by the fences surrounding this call in blk.rs.
    unsafe {
        let avail = core::ptr::addr_of_mut!((*AVAIL.0.get()).avail);
        let idx = core::ptr::read_volatile(core::ptr::addr_of!((*avail).idx));
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!((*avail).ring[idx as usize % NUM]),
            head,
        );
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        core::ptr::write_volatile(core::ptr::addr_of_mut!((*avail).idx), idx.wrapping_add(1));
    }
}

pub fn used_idx() -> u16 {
    // SAFETY: caller holds the disk lock and executes an I/O fence before
    // consuming entries written by the device.
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*USED.0.get()).used.idx)) }
}

pub fn used_id(index: u16) -> u32 {
    // SAFETY: as used_idx; index names an entry already published by the
    // device through used.idx.
    unsafe {
        core::ptr::read_volatile(core::ptr::addr_of!(
            (*USED.0.get()).used.ring[index as usize % NUM].id
        ))
    }
}
