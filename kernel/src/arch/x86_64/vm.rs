//! Four-level x86_64 page tables and the adapter's identity map.

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::arch::{KSTACK_PAGES, PAGE_SIZE};
use crate::mm::addr::{PhysAddr, VirtAddr, px};
use crate::mm::frame::PhysFrame;
use crate::mm::kalloc;
use crate::params::{NCPU, NPROC};

const PTE_P: u64 = 1 << 0;
const PTE_W: u64 = 1 << 1;
const PTE_U: u64 = 1 << 2;
const PTE_PS: u64 = 1 << 7;
const PTE_X_MARK: u64 = 1 << 9;
const PTE_NX: u64 = 1 << 63;
const PTE_ADDR: u64 = 0x000f_ffff_ffff_f000;
static KERNEL_ROOT: AtomicU64 = AtomicU64::new(0);
const HUGE_SIZE: u64 = 2 * 1024 * 1024;

pub const MAXVA: u64 = 1 << 38;
pub const TRAMPOLINE: VirtAddr = VirtAddr(MAXVA - PAGE_SIZE as u64);
pub const TRAPFRAME: VirtAddr = VirtAddr(TRAMPOLINE.0 - PAGE_SIZE as u64);
pub(super) const KERNEL_HIGH_BASE: u64 = 0xffff_8000_0000_0000;
pub(super) const TRAP_ENTRY_VA: u64 = KERNEL_HIGH_BASE;

pub const fn kstack(p: usize) -> VirtAddr {
    let stride = (KSTACK_PAGES + 1) as u64 * PAGE_SIZE as u64;
    VirtAddr(KERNEL_HIGH_BASE + 4 * 1024 * 1024 + p as u64 * stride)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Perm(u64);

impl Perm {
    pub const R: Perm = Perm(0);
    pub const W: Perm = Perm(PTE_W);
    pub const X: Perm = Perm(PTE_X_MARK);
    pub const U: Perm = Perm(PTE_U);

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn from_pte_flags(flags: u64) -> Perm {
        Perm(flags & (PTE_W | PTE_U | PTE_X_MARK))
    }
}

impl core::ops::BitOr for Perm {
    type Output = Perm;

    fn bitor(self, rhs: Perm) -> Perm {
        Perm(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for Perm {
    fn bitor_assign(&mut self, rhs: Perm) {
        self.0 |= rhs.0;
    }
}

pub fn pte_writable(pte: u64) -> bool {
    pte & PTE_W != 0
}

pub const fn pte_addr(pte: u64) -> PhysAddr {
    PhysAddr(pte & PTE_ADDR)
}

pub struct PageTable {
    root: PhysAddr,
}

impl PageTable {
    pub fn new() -> Option<Self> {
        let root = kalloc::alloc()?.leak();
        zero(root);
        Some(Self { root })
    }

    pub fn map_range(
        &mut self,
        va: VirtAddr,
        pa: PhysAddr,
        size: u64,
        perm: Perm,
    ) -> Result<(), ()> {
        let page = PAGE_SIZE as u64;
        assert!(va.0.is_multiple_of(page), "mappages: va not aligned");
        assert!(pa.0.is_multiple_of(page), "mappages: pa not aligned");
        assert!(size != 0 && size.is_multiple_of(page), "mappages: size");
        let last = va.0.checked_add(size - page).expect("mappages: overflow");
        assert!(
            valid_va(va.0) && valid_va(last) && (va.0 < MAXVA) == (last < MAXVA),
            "walk: non-canonical range"
        );
        let mut a = va.0;
        let mut p = pa.0;
        loop {
            let slot = walk(self.root, a, true).ok_or(())?;
            // SAFETY: walk returned a leaf slot owned by this page-table tree.
            unsafe {
                if *slot & PTE_P != 0 {
                    panic!("mappages: remap");
                }
                *slot = p | leaf_flags(perm);
            }
            if a == last {
                return Ok(());
            }
            a += page;
            p += page;
        }
    }

    pub fn leaf_pte(&self, va: u64) -> Option<u64> {
        let slot = walk(self.root, va, false)?;
        // SAFETY: the tree owns the returned slot and this is an unaliased read.
        let pte = unsafe { *slot };
        (pte & PTE_P != 0).then_some(pte)
    }

    pub fn clear_user(&mut self, va: u64) {
        let slot = walk(self.root, va, false).expect("uvmclear");
        // SAFETY: exclusive access to this table owns the leaf slot.
        unsafe {
            assert!(*slot & PTE_P != 0, "uvmclear");
            *slot &= !PTE_U;
        }
    }

    pub fn take_leaf(&mut self, va: u64) -> Option<PhysAddr> {
        let slot = walk(self.root, va, false)?;
        // SAFETY: exclusive table access owns the leaf slot.
        let pte = unsafe { *slot };
        if pte & PTE_P == 0 {
            return None;
        }
        // SAFETY: same exclusive slot ownership.
        unsafe { *slot = 0 };
        Some(pte_addr(pte))
    }

    pub fn take_next_leaf(&mut self, start: u64, end: u64) -> Option<(u64, PhysAddr)> {
        let va = next_leaf(self.root, 3, 0, start, end)?;
        let pa = self.take_leaf(va).expect("next leaf disappeared");
        Some((va, pa))
    }

    pub fn cr3_value(&self) -> u64 {
        self.root.0
    }

    pub fn walkaddr(&self, va: u64) -> Option<PhysAddr> {
        if va >= MAXVA {
            return None;
        }
        let pte = self.leaf_pte(va)?;
        (pte & PTE_U != 0).then_some(pte_addr(pte))
    }

    pub fn leak_root(self) -> PhysAddr {
        let root = self.root;
        core::mem::forget(self);
        root
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        freewalk(self.root, 3);
    }
}

fn leaf_flags(perm: Perm) -> u64 {
    PTE_P
        | perm.bits()
        | if perm.bits() & PTE_X_MARK == 0 {
            PTE_NX
        } else {
            0
        }
}

const fn valid_va(va: u64) -> bool {
    va < MAXVA || va >= KERNEL_HIGH_BASE
}

fn walk(root: PhysAddr, va: u64, alloc: bool) -> Option<*mut u64> {
    assert!(valid_va(va), "walk: non-canonical va");
    let mut table = table_of(root);
    for level in (1..=3).rev() {
        let index = px(level, va);
        let pte = pte_read(table, index);
        if pte & PTE_P != 0 {
            assert!(pte & PTE_PS == 0, "walk: huge page branch");
            table = table_of(pte_addr(pte));
            continue;
        }
        if !alloc {
            return None;
        }
        let child = kalloc::alloc()?.leak();
        zero(child);
        pte_write(table, index, child.0 | PTE_P | PTE_W | PTE_U);
        table = table_of(child);
    }
    // SAFETY: forming the address of a slot in the owned leaf table.
    Some(unsafe { core::ptr::addr_of_mut!((*table)[px(0, va)]) })
}

fn map_huge(root: PhysAddr, start: u64, end: u64, perm: Perm) -> Result<(), ()> {
    assert!(start.is_multiple_of(HUGE_SIZE) && end.is_multiple_of(HUGE_SIZE));
    let mut va = start;
    while va < end {
        let mut table = table_of(root);
        for level in (2..=3).rev() {
            let index = px(level, va);
            let pte = pte_read(table, index);
            if pte & PTE_P == 0 {
                let child = kalloc::alloc().ok_or(())?.leak();
                zero(child);
                pte_write(table, index, child.0 | PTE_P | PTE_W | PTE_U);
                table = table_of(child);
            } else {
                assert!(pte & PTE_PS == 0, "huge map branch");
                table = table_of(pte_addr(pte));
            }
        }
        let index = px(1, va);
        assert!(pte_read(table, index) & PTE_P == 0, "huge map remap");
        pte_write(table, index, va | leaf_flags(perm) | PTE_PS);
        va += HUGE_SIZE;
    }
    Ok(())
}

fn next_leaf(table_pa: PhysAddr, level: u32, base: u64, start: u64, end: u64) -> Option<u64> {
    if start >= end {
        return None;
    }
    let table = table_of(table_pa);
    let span = 1u64 << (12 + level * 9);
    for index in 0..512 {
        let entry_base = base + index as u64 * span;
        let entry_end = entry_base + span;
        if entry_end <= start {
            continue;
        }
        if entry_base >= end {
            break;
        }
        let pte = pte_read(table, index);
        if pte & PTE_P == 0 {
            continue;
        }
        if level == 0 {
            return Some(entry_base);
        }
        if pte & PTE_PS != 0 {
            assert!(pte & PTE_U == 0, "user huge page");
            continue;
        }
        if let Some(va) = next_leaf(pte_addr(pte), level - 1, entry_base, start, end) {
            return Some(va);
        }
    }
    None
}

fn freewalk(root: PhysAddr, level: u32) {
    let table = table_of(root);
    for index in 0..512 {
        let pte = pte_read(table, index);
        if pte & PTE_P == 0 {
            continue;
        }
        if level > 0 && pte & PTE_PS == 0 {
            freewalk(pte_addr(pte), level - 1);
            pte_write(table, index, 0);
        } else if pte & PTE_U != 0 {
            panic!("freewalk: user leaf");
        }
    }
    // SAFETY: recursive freewalk removed every owned child; `root` is the
    // unique frame leaked into this page-table node by PageTable.
    drop(unsafe { PhysFrame::from_raw(root) });
}

fn table_of(pa: PhysAddr) -> *mut [u64; 512] {
    pa.0 as usize as *mut [u64; 512]
}

fn pte_read(table: *mut [u64; 512], index: usize) -> u64 {
    // SAFETY: page-table pages are exclusively owned by their tree.
    unsafe { (*table)[index] }
}

fn pte_write(table: *mut [u64; 512], index: usize, pte: u64) {
    // SAFETY: page-table pages are exclusively owned by their tree.
    unsafe { (*table)[index] = pte };
}

fn zero(pa: PhysAddr) {
    // SAFETY: pa is a freshly allocated page-table frame.
    unsafe { core::ptr::write_bytes(pa.0 as usize as *mut u8, 0, PAGE_SIZE) };
}

unsafe extern "C" {
    static etext: u8;
}

fn map_kernel_identity(pt: &mut PageTable) -> Result<(), ()> {
    // SAFETY: linker symbols are used only for their numeric addresses.
    let start = super::KERNBASE.0;
    let text_end = (&raw const etext) as u64;
    pt.map_range(
        VirtAddr(start),
        PhysAddr(start),
        text_end - start,
        Perm::R | Perm::X,
    )?;
    pt.map_range(
        VirtAddr(text_end),
        PhysAddr(text_end),
        HUGE_SIZE - text_end,
        Perm::R | Perm::W,
    )?;
    map_huge(pt.root, HUGE_SIZE, 128 * 1024 * 1024, Perm::R | Perm::W)?;
    map_huge(pt.root, 0xc000_0000, 0x1_0000_0000, Perm::R | Perm::W)
}

fn map_cpu_tables(pt: &mut PageTable) -> Result<(), ()> {
    for cpu in 0..NCPU {
        pt.map_range(
            VirtAddr(super::gdt::gdt_va(cpu)),
            PhysAddr(super::gdt::gdt_addr(cpu)),
            PAGE_SIZE as u64,
            Perm::R | Perm::W,
        )?;
        pt.map_range(
            VirtAddr(super::gdt::tss_va(cpu)),
            PhysAddr(super::gdt::tss_addr(cpu)),
            PAGE_SIZE as u64,
            Perm::R | Perm::W,
        )?;
    }
    Ok(())
}

fn map_kernel(pt: &mut PageTable) {
    map_kernel_identity(pt).expect("x86 kernel map");
    pt.map_range(
        VirtAddr(super::boot::AP_BOOT_ADDR as u64),
        PhysAddr(super::boot::AP_BOOT_ADDR as u64),
        PAGE_SIZE as u64,
        Perm::R | Perm::W,
    )
    .expect("x86 AP bootstrap map");
    pt.map_range(
        VirtAddr(super::traps::IDT_VA),
        PhysAddr(super::traps::idt_addr()),
        PAGE_SIZE as u64,
        Perm::R,
    )
    .expect("x86 IDT map");
    pt.map_range(
        VirtAddr(TRAP_ENTRY_VA),
        PhysAddr(trampoline_addr()),
        PAGE_SIZE as u64,
        Perm::R | Perm::X,
    )
    .expect("x86 trap entry map");
    map_cpu_tables(pt).expect("x86 GDT/TSS map");
}

unsafe extern "C" {
    static X86_KERNEL_CR3: AtomicU64;
}

fn set_kernel_root(root: PhysAddr) {
    // SAFETY: the trampoline owns this atomic word and the BSP publishes it once.
    unsafe { X86_KERNEL_CR3.store(root.0, Ordering::Release) };
}

/// Build and publish the shared x86_64 kernel page table.
pub fn init_kernel_table() {
    let mut pt = PageTable::new().expect("kvmmake: no page for root");
    map_kernel(&mut pt);
    pt.map_range(
        TRAMPOLINE,
        PhysAddr(trampoline_addr()),
        PAGE_SIZE as u64,
        Perm::R | Perm::X,
    )
    .expect("trampoline map");
    for slot in 0..NPROC {
        for stack_page in 0..KSTACK_PAGES {
            let frame = kalloc::alloc().expect("proc_mapstacks: kalloc");
            pt.map_range(
                VirtAddr(kstack(slot).0 + stack_page as u64 * PAGE_SIZE as u64),
                frame.leak(),
                PAGE_SIZE as u64,
                Perm::R | Perm::W,
            )
            .expect("kernel stack map");
        }
    }
    let root = pt.leak_root();
    set_kernel_root(root);
    KERNEL_ROOT.store(root.0, Ordering::Release);
}

/// Activate the published kernel table on this hart.
pub fn activate_kernel_table() {
    let root = KERNEL_ROOT.load(Ordering::Acquire);
    assert!(root != 0, "kvminithart before kvminit");
    activate(PhysAddr(root));
}

pub fn prepare_user_table(pt: &mut PageTable, slot: usize) -> Result<(), ()> {
    // SAFETY: the BSP published the kernel root before allocating processes.
    let root = PhysAddr(unsafe { X86_KERNEL_CR3.load(Ordering::Acquire) });
    assert!(root.0 != 0, "user table before kernel table");
    pt.map_range(
        VirtAddr(super::traps::IDT_VA),
        PhysAddr(super::traps::idt_addr()),
        PAGE_SIZE as u64,
        Perm::R,
    )?;
    pt.map_range(
        VirtAddr(TRAP_ENTRY_VA),
        PhysAddr(trampoline_addr()),
        PAGE_SIZE as u64,
        Perm::R | Perm::X,
    )?;
    map_cpu_tables(pt)?;
    for page in 0..KSTACK_PAGES {
        let va = kstack(slot).0 + page as u64 * PAGE_SIZE as u64;
        let pa = lookup_any(root, va).expect("kernel stack mapping");
        pt.map_range(VirtAddr(va), pa, PAGE_SIZE as u64, Perm::R | Perm::W)?;
    }
    Ok(())
}

fn lookup_any(root: PhysAddr, va: u64) -> Option<PhysAddr> {
    let slot = walk(root, va, false)?;
    // SAFETY: walk returned a live kernel table slot.
    let pte = unsafe { *slot };
    (pte & PTE_P != 0).then_some(pte_addr(pte))
}

pub fn activate(root: PhysAddr) {
    // SAFETY: root names a live PML4 whose kernel mappings include this code and stack.
    unsafe { asm!("mov cr3, {}", in(reg) root.0, options(nostack, preserves_flags)) };
}

pub fn trampoline_addr() -> u64 {
    super::traps::entry_addr() & !((PAGE_SIZE as u64) - 1)
}
