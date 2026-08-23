//! Virtio-MMIO v2 transport for QEMU's `virt` block device.

use crate::mm::layout::VIRTIO0;

const BASE: usize = VIRTIO0.0 as usize;

const MAGIC_VALUE: usize = 0x000;
const VERSION: usize = 0x004;
const DEVICE_ID: usize = 0x008;
const VENDOR_ID: usize = 0x00c;
const DEVICE_FEATURES: usize = 0x010;
const DEVICE_FEATURES_SEL: usize = 0x014;
const DRIVER_FEATURES: usize = 0x020;
const DRIVER_FEATURES_SEL: usize = 0x024;
const QUEUE_SEL: usize = 0x030;
const QUEUE_NUM_MAX: usize = 0x034;
const QUEUE_NUM: usize = 0x038;
const QUEUE_READY: usize = 0x044;
const QUEUE_NOTIFY: usize = 0x050;
const INTERRUPT_STATUS: usize = 0x060;
const INTERRUPT_ACK: usize = 0x064;
const STATUS: usize = 0x070;
const QUEUE_DESC_LOW: usize = 0x080;
const QUEUE_DESC_HIGH: usize = 0x084;
const QUEUE_AVAIL_LOW: usize = 0x090;
const QUEUE_AVAIL_HIGH: usize = 0x094;
const QUEUE_USED_LOW: usize = 0x0a0;
const QUEUE_USED_HIGH: usize = 0x0a4;

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

const BLK_F_RO: u32 = 5;
const BLK_F_SCSI: u32 = 7;
const BLK_F_FLUSH: u32 = 9;
const BLK_F_CONFIG_WCE: u32 = 11;
const BLK_F_MQ: u32 = 12;
const F_ANY_LAYOUT: u32 = 27;
const RING_F_INDIRECT_DESC: u32 = 28;
const RING_F_EVENT_IDX: u32 = 29;

fn read(offset: usize) -> u32 {
    // SAFETY: one aligned volatile read from the fixed virtio-MMIO window.
    unsafe { core::ptr::read_volatile((BASE + offset) as *const u32) }
}

fn write(offset: usize, value: u32) {
    // SAFETY: one aligned volatile write to the fixed virtio-MMIO window.
    unsafe { core::ptr::write_volatile((BASE + offset) as *mut u32, value) }
}

pub fn init(desc: u64, avail: u64, used: u64, queue_len: u32) {
    if read(MAGIC_VALUE) != 0x7472_6976
        || read(VERSION) != 2
        || read(DEVICE_ID) != 2
        || read(VENDOR_ID) != 0x554d_4551
    {
        panic!("could not find virtio disk");
    }

    let mut status = 0;
    write(STATUS, status);
    status |= STATUS_ACKNOWLEDGE;
    write(STATUS, status);
    status |= STATUS_DRIVER;
    write(STATUS, status);

    write(DEVICE_FEATURES_SEL, 0);
    let mut low = read(DEVICE_FEATURES);
    for bit in [
        BLK_F_RO,
        BLK_F_SCSI,
        BLK_F_FLUSH,
        BLK_F_CONFIG_WCE,
        BLK_F_MQ,
        F_ANY_LAYOUT,
        RING_F_INDIRECT_DESC,
        RING_F_EVENT_IDX,
    ] {
        low &= !(1 << bit);
    }
    write(DRIVER_FEATURES_SEL, 0);
    write(DRIVER_FEATURES, low);
    write(DEVICE_FEATURES_SEL, 1);
    let high = read(DEVICE_FEATURES);
    write(DRIVER_FEATURES_SEL, 1);
    write(DRIVER_FEATURES, high);

    status |= STATUS_FEATURES_OK;
    write(STATUS, status);
    if read(STATUS) & STATUS_FEATURES_OK == 0 {
        panic!("virtio disk FEATURES_OK unset");
    }

    write(QUEUE_SEL, 0);
    if read(QUEUE_READY) != 0 {
        panic!("virtio disk queue already ready");
    }
    let max = read(QUEUE_NUM_MAX);
    if max == 0 || max < queue_len {
        panic!("virtio disk queue too short");
    }
    write(QUEUE_NUM, queue_len);
    write_addr(QUEUE_DESC_LOW, QUEUE_DESC_HIGH, desc);
    write_addr(QUEUE_AVAIL_LOW, QUEUE_AVAIL_HIGH, avail);
    write_addr(QUEUE_USED_LOW, QUEUE_USED_HIGH, used);
    write(QUEUE_READY, 1);

    status |= STATUS_DRIVER_OK;
    write(STATUS, status);
}

fn write_addr(low: usize, high: usize, address: u64) {
    write(low, address as u32);
    write(high, (address >> 32) as u32);
}

pub fn notify() {
    write(QUEUE_NOTIFY, 0);
}

pub fn acknowledge_interrupt() {
    write(INTERRUPT_ACK, read(INTERRUPT_STATUS) & 0x3);
}
