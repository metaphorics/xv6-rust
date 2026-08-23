//! Bus-0 PCI discovery and the modern virtio-pci queue transport.

use core::ptr;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::{inl, outl};

const VIRTIO_VENDOR: u16 = 0x1af4;
const VIRTIO_BLK_MODERN: u16 = 0x1042;
const PCI_CAP_VENDOR: u8 = 0x09;
const CFG_COMMON: u8 = 1;
const CFG_NOTIFY: u8 = 2;
const CFG_ISR: u8 = 3;

const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_DRIVER_OK: u8 = 4;
const STATUS_FEATURES_OK: u8 = 8;

static NOTIFY: AtomicU64 = AtomicU64::new(0);
static ISR: AtomicU64 = AtomicU64::new(0);
static IRQ: AtomicU32 = AtomicU32::new(u32::MAX);

fn config_address(bdf: u32, offset: u8) -> u32 {
    0x8000_0000 | bdf | u32::from(offset & 0xfc)
}

fn config_read32(bdf: u32, offset: u8) -> u32 {
    outl(0xcf8, config_address(bdf, offset));
    inl(0xcfc)
}

fn config_write32(bdf: u32, offset: u8, value: u32) {
    outl(0xcf8, config_address(bdf, offset));
    outl(0xcfc, value);
}

fn config_read8(bdf: u32, offset: u8) -> u8 {
    (config_read32(bdf, offset) >> (u32::from(offset & 3) * 8)) as u8
}

fn config_read16(bdf: u32, offset: u8) -> u16 {
    (config_read32(bdf, offset) >> (u32::from(offset & 2) * 8)) as u16
}

fn config_write16(bdf: u32, offset: u8, value: u16) {
    let shift = u32::from(offset & 2) * 8;
    let old = config_read32(bdf, offset);
    config_write32(
        bdf,
        offset,
        (old & !(0xffff << shift)) | (u32::from(value) << shift),
    );
}

fn find_device() -> Option<u32> {
    for device in 0..32u32 {
        for function in 0..8u32 {
            let bdf = (device << 11) | (function << 8);
            let id = config_read32(bdf, 0);
            if id as u16 == VIRTIO_VENDOR && (id >> 16) as u16 == VIRTIO_BLK_MODERN {
                return Some(bdf);
            }
            if function == 0 && id as u16 == 0xffff {
                break;
            }
        }
    }
    None
}

fn bar_address(bdf: u32, bar: u8) -> u64 {
    let offset = 0x10 + bar * 4;
    let low = config_read32(bdf, offset);
    assert!(low & 1 == 0, "virtio pci I/O BAR");
    let mut address = u64::from(low & !0xf);
    if low & 0x6 == 0x4 {
        address |= u64::from(config_read32(bdf, offset + 4)) << 32;
    }
    address
}

#[derive(Clone, Copy)]
struct Capability {
    base: u64,
    multiplier: u32,
}

fn capabilities(bdf: u32) -> [Option<Capability>; 4] {
    assert!(
        config_read16(bdf, 0x06) & 0x10 != 0,
        "virtio pci capabilities"
    );
    let mut result = [None; 4];
    let mut next = config_read8(bdf, 0x34) & !3;
    let mut remaining = 48;
    while next != 0 && remaining > 0 {
        remaining -= 1;
        let cap = next;
        next = config_read8(bdf, cap + 1) & !3;
        if config_read8(bdf, cap) != PCI_CAP_VENDOR || config_read8(bdf, cap + 2) < 16 {
            continue;
        }
        let kind = config_read8(bdf, cap + 3);
        if usize::from(kind) >= result.len() {
            continue;
        }
        let bar = config_read8(bdf, cap + 4);
        let offset = u64::from(config_read32(bdf, cap + 8));
        let multiplier = if kind == CFG_NOTIFY {
            config_read32(bdf, cap + 16)
        } else {
            0
        };
        result[usize::from(kind)] = Some(Capability {
            base: bar_address(bdf, bar) + offset,
            multiplier,
        });
    }
    result
}

fn read8(base: u64, offset: usize) -> u8 {
    // SAFETY: base came from a validated vendor capability in a mapped MMIO BAR.
    unsafe { ptr::read_volatile((base as usize + offset) as *const u8) }
}

fn read16(base: u64, offset: usize) -> u16 {
    // SAFETY: as read8; common config fields are naturally aligned.
    unsafe { ptr::read_volatile((base as usize + offset) as *const u16) }
}

fn read32(base: u64, offset: usize) -> u32 {
    // SAFETY: as read8; common config fields are naturally aligned.
    unsafe { ptr::read_volatile((base as usize + offset) as *const u32) }
}

fn write8(base: u64, offset: usize, value: u8) {
    // SAFETY: as read8; this writes a defined common-config field.
    unsafe { ptr::write_volatile((base as usize + offset) as *mut u8, value) };
}

fn write16(base: u64, offset: usize, value: u16) {
    // SAFETY: as read8; this writes a defined common-config field.
    unsafe { ptr::write_volatile((base as usize + offset) as *mut u16, value) };
}

fn write32(base: u64, offset: usize, value: u32) {
    // SAFETY: as read8; this writes a defined common-config field.
    unsafe { ptr::write_volatile((base as usize + offset) as *mut u32, value) };
}

fn write64(base: u64, offset: usize, value: u64) {
    // SAFETY: as read8; queue addresses are aligned 64-bit fields.
    unsafe { ptr::write_volatile((base as usize + offset) as *mut u64, value) };
}

pub fn init(desc: u64, avail: u64, used: u64, queue_size: u32) {
    let bdf = find_device().expect("virtio pci device 1af4:1042");
    let command = (config_read16(bdf, 0x04) | 0x0006) & !(1 << 10);
    config_write16(bdf, 0x04, command);

    let caps = capabilities(bdf);
    let common = caps[usize::from(CFG_COMMON)].expect("virtio common capability");
    let notify = caps[usize::from(CFG_NOTIFY)].expect("virtio notify capability");
    let isr = caps[usize::from(CFG_ISR)].expect("virtio ISR capability");

    write8(common.base, 20, 0);
    write8(common.base, 20, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
    write32(common.base, 0, 1);
    assert!(read32(common.base, 4) & 1 != 0, "virtio VERSION_1");
    write32(common.base, 8, 0);
    write32(common.base, 12, 0);
    write32(common.base, 8, 1);
    write32(common.base, 12, 1);
    write8(
        common.base,
        20,
        STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK,
    );
    assert!(
        read8(common.base, 20) & STATUS_FEATURES_OK != 0,
        "virtio features"
    );

    write16(common.base, 22, 0);
    assert!(
        u32::from(read16(common.base, 24)) >= queue_size,
        "virtio queue too small"
    );
    write16(common.base, 24, queue_size as u16);
    write64(common.base, 32, desc);
    write64(common.base, 40, avail);
    write64(common.base, 48, used);
    let notify_offset = u64::from(read16(common.base, 30));
    write16(common.base, 28, 1);

    NOTIFY.store(
        notify.base + notify_offset * u64::from(notify.multiplier),
        Ordering::Release,
    );
    ISR.store(isr.base, Ordering::Release);
    let pin = u32::from(config_read8(bdf, 0x3d));
    assert!((1..=4).contains(&pin), "virtio pci interrupt pin");
    let slot = (bdf >> 11) & 0x1f;
    let irq = 20 + ((slot + pin - 1) & 3);
    IRQ.store(irq, Ordering::Release);
    super::intr::route(irq);

    write8(
        common.base,
        20,
        STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
    );
}

pub fn notify() {
    let address = NOTIFY.load(Ordering::Acquire);
    assert!(address != 0, "virtio notify before init");
    // SAFETY: init derived the queue-0 doorbell from the notify capability.
    unsafe { ptr::write_volatile(address as usize as *mut u16, 0) };
}

pub fn acknowledge_interrupt() {
    let address = ISR.load(Ordering::Acquire);
    if address != 0 {
        // SAFETY: reading the ISR capability acknowledges the device interrupt.
        let _ = unsafe { ptr::read_volatile(address as usize as *const u8) };
    }
}

pub fn interrupt_line() -> Option<u32> {
    let irq = IRQ.load(Ordering::Acquire);
    (irq != u32::MAX).then_some(irq)
}
