//! The process table, scheduler, and lifecycle (`kernel/proc.c`,
//! `kernel/proc.h`).
//!
//! ## Ownership split (proc.h's lock-ownership comments, normative)
//!
//! - `shared: SpinLock<ProcShared>` — exactly the fields proc.h:85-90
//!   puts under `p->lock`: `state`, `chan`, `killed`, `xstate`, `pid`.
//! - `parent` — mutated only under the global [`WAIT_LOCK`]
//!   (proc.h:92-93); stored as a table index rather than a pointer.
//! - `private: ProcPrivate` — proc.h:95-103's "private to the process"
//!   fields (size, page table, trapframe, context, name). Reached only
//!   through a [`CurrentProc`] token (the running process) or while a
//!   caller holds the slot's `shared` lock with state `Used` (the
//!   allocproc/freeproc/fork paths).
//!
//! ## Lock ordering
//!
//! [`WAIT_LOCK`] must always be acquired BEFORE any `shared` lock
//! (proc.c:23-27). Never hold two `shared` locks except the parent scan
//! in `wait` (wait_lock held, one child at a time).
//!
//! ## The switch handoff
//!
//! `p.shared` is deliberately held across `swtch`: the scheduler
//! acquires it, switches into the process, and the process (or the next
//! scheduler pass) releases it. Because `swtch` runs on one hart, lock
//! ownership (per-hart in xv6) travels with the flow. Rust guards
//! cannot cross a context switch, so the handoff points use
//! [`SpinLock::release_raw`] on a forgotten guard — every use is
//! paired and documented.

use core::cell::UnsafeCell;
use core::mem;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use crate::arch::{
    self, Context, KSTACK_PAGES, PAGE_SIZE, PageTable, Perm, TRAMPOLINE, TRAPFRAME, TrapFrame,
    kstack,
};
use crate::cpu;
use crate::err::Err;
use crate::fs::file::FileHandle;
use crate::fs::inode::Inode;
use crate::mm::addr::{PhysAddr, VirtAddr};
use crate::mm::frame::PhysFrame;
use crate::mm::kalloc;
use crate::mm::uvm;
use crate::params::{NOFILE, NPROC};
use crate::sync::{SpinGuard, SpinLock, pop_off, push_off};

/// Helps ensure that wakeups of `wait()`ing parents are not lost; must
/// be acquired before any `shared` lock (`wait_lock`, proc.c:23-27).
static WAIT_LOCK: SpinLock<()> = SpinLock::new(());

/// The next process id (`nextpid`, proc.c:15); the atomic's
/// `fetch_add` is `allocpid` (proc.c:92-103).
static NEXT_PID: AtomicI32 = AtomicI32::new(1);

/// The first process's slot, for reparenting (`initproc`, proc.c:13).
/// `usize::MAX` until [`user_init`] runs.
static INITPROC: AtomicUsize = AtomicUsize::new(usize::MAX);
/// File-system initialization runs once in process context because disk I/O
/// may sleep (`first`, proc.c:516-529).
static FS_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// `parent` value meaning "no parent" (C's null pointer).
const PARENT_NONE: usize = usize::MAX;

/// Process state (`enum procstate`, proc.h:79).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    Unused,
    Used,
    Sleeping,
    Runnable,
    Running,
    Zombie,
}

/// The fields under `p->lock` (proc.h:85-90).
pub struct ProcShared {
    pub state: ProcState,
    /// Sleep channel. If non-zero, sleeping on this address (proc.h:87).
    pub chan: usize,
    /// If non-zero, have been killed (proc.h:88).
    pub killed: bool,
    /// Exit status for the parent's `wait` (proc.h:89).
    pub xstate: i32,
    /// Process id (proc.h:90).
    pub pid: i32,
}

/// The process-private fields (proc.h:95-103).
pub struct ProcPrivate {
    /// Size of process memory in bytes (proc.h:97).
    pub sz: u64,
    /// The trapframe page this slot owns; the `*mut TrapFrame` view into
    /// it is derived on demand (proc.h:99's pointer, kept as the owning
    /// handle so double frees are unrepresentable).
    pub trapframe_page: Option<PhysFrame>,
    /// User page table (proc.h:98). `None` while the slot is unused.
    pub pagetable: Option<PageTable>,
    /// Kernel context switch state (proc.h:100).
    pub context: Context,
    /// Process name, nul-terminated (proc.h:103).
    pub name: [u8; 16],
    /// Current working directory (`proc.h:102`).
    pub cwd: Option<Inode>,
    /// Open-file descriptors (`proc.h:101`).
    pub ofile: [Option<FileHandle>; NOFILE],
}

impl ProcPrivate {
    /// The top of this slot's multi-page kernel stack.
    fn kstack_top(slot: usize) -> u64 {
        kstack(slot).0 + (KSTACK_PAGES * PAGE_SIZE) as u64
    }

    /// The trapframe view into this slot's page.
    fn trapframe(&mut self) -> &mut TrapFrame {
        let pa = self.trapframe_page.as_ref().expect("trapframe page").addr();
        // SAFETY: the page is owned by this slot (the PhysFrame above)
        // and touched only through this process's single running flow —
        // the private-fields discipline of proc.h:95-103.
        unsafe { &mut *(pa.0 as usize as *mut TrapFrame) }
    }
}

/// One process slot (`struct proc`, proc.h:82-104). `kstack` is not
/// stored: it is `KSTACK(slot)`, a pure function of the slot index
/// (proc.c:57).
pub struct Proc {
    /// The `p->lock` scope (proc.h:83, 85-90).
    shared: SpinLock<ProcShared>,
    /// Parent slot, under [`WAIT_LOCK`] only (proc.h:92-93).
    parent: UnsafeCell<usize>,
    /// Process-private fields (proc.h:95-103). Running-process access is
    /// serialized by `private_borrowed`; lifecycle paths access it only
    /// while holding `shared` with a non-running state.
    private: UnsafeCell<ProcPrivate>,
    private_borrowed: AtomicBool,
}

// SAFETY: `shared` is a SpinLock (Sync for Send payloads); `parent` is
// written only under WAIT_LOCK, one writer at a time; `private` is
// reached either by one runtime-checked running-process borrow or under
// the slot's `shared` lock while the process is not running.
unsafe impl Sync for Proc {}

/// The process table (`proc[NPROC]`, proc.c:11). Const-constructed: C's
/// `procinit` (proc.c:47-59) only initializes locks and kstack values,
/// both of which are const here. The inline const creates distinct
/// interior-mutable cells for every slot.
static PROCS: [Proc; NPROC] = [const {
    Proc {
        shared: SpinLock::new(ProcShared {
            state: ProcState::Unused,
            chan: 0,
            killed: false,
            xstate: 0,
            pid: 0,
        }),
        parent: UnsafeCell::new(PARENT_NONE),
        private: UnsafeCell::new(ProcPrivate {
            sz: 0,
            trapframe_page: None,
            pagetable: None,
            context: Context::ZERO,
            name: [0; 16],
            cwd: None,
            ofile: [const { None }; NOFILE],
        }),
        private_borrowed: AtomicBool::new(false),
    }
}; NPROC];

/// A token naming the process running on this hart (`myproc`, proc.c:82-90).
///
/// Tokens may be obtained repeatedly, but access to process-private state
/// goes through a runtime-checked guard. Consequently repeated `my_proc()`
/// calls cannot mint overlapping Rust references.
pub struct CurrentProc {
    slot: usize,
}

/// The process running on this hart, if any (`myproc`, proc.c:82-90).
pub fn my_proc() -> Option<CurrentProc> {
    push_off();
    let slot = cpu::current().current_slot();
    pop_off();
    slot.map(|slot| CurrentProc { slot })
}

struct PrivateGuard<'a> {
    proc: &'a Proc,
}

impl core::ops::Deref for PrivateGuard<'_> {
    type Target = ProcPrivate;

    fn deref(&self) -> &Self::Target {
        // SAFETY: construction acquired `private_borrowed`, and Drop
        // releases it only after every reference derived from this guard.
        unsafe { private(self.proc) }
    }
}

impl core::ops::DerefMut for PrivateGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: as Deref; the guard is the unique private-state borrower.
        unsafe { &mut *private_ptr(self.proc) }
    }
}

impl Drop for PrivateGuard<'_> {
    fn drop(&mut self) {
        // Normal unwinds and context-switch paths drop the guard before
        // execution continues elsewhere. The kernel uses panic=abort, so a
        // panic runs no destructors and has no post-panic execution that
        // could observe the borrow bit still set.
        assert!(
            self.proc.private_borrowed.swap(false, Ordering::Release),
            "current process private borrow was not held"
        );
    }
}

/// A scoped borrow of the current process's user-memory state.
pub struct UserMemory<'a> {
    private: PrivateGuard<'a>,
}

impl UserMemory<'_> {
    pub fn size(&self) -> u64 {
        self.private.sz
    }

    pub fn set_size(&mut self, size: u64) {
        self.private.sz = size;
    }
    pub fn pagetable(&self) -> &PageTable {
        self.private.pagetable.as_ref().expect("proc pagetable")
    }

    pub fn pagetable_mut(&mut self) -> &mut PageTable {
        self.private.pagetable.as_mut().expect("proc pagetable")
    }
}

/// A scoped exclusive borrow of the current process's saved registers.
pub struct TrapFrameGuard<'a> {
    private: PrivateGuard<'a>,
}

impl core::ops::Deref for TrapFrameGuard<'_> {
    type Target = TrapFrame;

    fn deref(&self) -> &Self::Target {
        let pa = self
            .private
            .trapframe_page
            .as_ref()
            .expect("trapframe page")
            .addr();
        // SAFETY: `private` is the unique running-process borrow and owns
        // this frame; the returned reference cannot outlive the guard.
        unsafe { &*(pa.0 as usize as *const TrapFrame) }
    }
}

impl core::ops::DerefMut for TrapFrameGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.private.trapframe()
    }
}

impl CurrentProc {
    /// The slot's table entry. The returned lifetime is tied to this token;
    /// safe private-state references require `private_guard` below.
    fn proc(&self) -> &Proc {
        &PROCS[self.slot]
    }

    fn private_guard(&self) -> PrivateGuard<'_> {
        assert!(
            self.proc()
                .private_borrowed
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok(),
            "overlapping current process private borrow"
        );
        PrivateGuard { proc: self.proc() }
    }

    /// Table slot index.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// This process's address, as a sleep channel: `wait` sleeps on the
    /// process's own address (`sleep_prepare(p)`, proc.c:414) and
    /// `exit` wakes it (`wakeup(p->parent)`, proc.c:353).
    pub fn chan_addr(&self) -> usize {
        self.proc() as *const Proc as usize
    }

    /// The shared-state lock (`&p->lock`).
    pub fn shared(&self) -> &SpinLock<ProcShared> {
        &self.proc().shared
    }

    /// Top of this process's kernel stack (`p->kstack + PGSIZE`).
    pub fn kstack_top(&self) -> u64 {
        ProcPrivate::kstack_top(self.slot)
    }

    /// Process id, read under the shared lock.
    pub fn pid(&self) -> i32 {
        self.shared().lock().pid
    }

    /// Has this process been killed? (`killed`, proc.c:627-634.)
    pub fn killed(&self) -> bool {
        self.shared().lock().killed
    }

    /// Mark this process killed (`setkilled`, proc.c:619-624).
    pub fn set_killed(&self) {
        self.shared().lock().killed = true;
    }

    /// Size of user memory (bytes).
    pub fn sz(&self) -> u64 {
        self.private_guard().sz
    }

    /// Update the user-memory size (`p->sz = sz`, growproc).
    pub fn set_sz(&self, sz: u64) {
        self.private_guard().sz = sz;
    }

    /// Borrow the page table and its matching size as one linear unit.
    pub fn user_memory(&self) -> UserMemory<'_> {
        UserMemory {
            private: self.private_guard(),
        }
    }

    /// Borrow the saved user register state for one lexical scope.
    pub fn trapframe(&self) -> TrapFrameGuard<'_> {
        TrapFrameGuard {
            private: self.private_guard(),
        }
    }

    /// Physical address of the stable trapframe page.
    ///
    /// This is an integer rather than a reference; architecture return
    /// assembly validates its use under the current-process invariant.
    pub fn trapframe_addr(&self) -> u64 {
        self.private_guard()
            .trapframe_page
            .as_ref()
            .expect("trapframe page")
            .addr()
            .0
    }

    /// The saved kernel context (`&p->context`, for `swtch`).
    fn context_ptr(&self) -> *mut Context {
        // SAFETY: no private guard crosses a scheduling operation; the raw
        // address is dereferenced only by `swtch` under the scheduler
        // discipline.
        unsafe { core::ptr::addr_of_mut!((*private_ptr(self.proc())).context) }
    }

    /// The name bytes (for fork's copy and diagnostics).
    pub fn name_bytes(&self) -> [u8; 16] {
        self.private_guard().name
    }

    /// The name as a displayable value, up to the first nul.
    pub fn name_str(&self) -> ProcName {
        ProcName(self.name_bytes())
    }

    pub fn cwd(&self) -> Option<Inode> {
        self.private_guard().cwd.as_ref().cloned()
    }

    pub fn set_cwd(&self, cwd: Option<Inode>) {
        self.private_guard().cwd = cwd;
    }

    pub fn file(&self, fd: usize) -> Option<FileHandle> {
        self.private_guard()
            .ofile
            .get(fd)
            .and_then(|file| file.as_ref().cloned())
    }

    pub fn replace_file(&self, fd: usize, file: Option<FileHandle>) -> Option<FileHandle> {
        let mut private = self.private_guard();
        let slot = private.ofile.get_mut(fd)?;
        core::mem::replace(slot, file)
    }

    /// Allocate an empty process page table sharing this process's
    /// trapframe page. Exec builds a replacement image in this table.
    pub fn new_exec_pagetable(&self) -> Option<PageTable> {
        proc_pagetable(self.slot, PhysAddr(self.trapframe_addr()))
    }

    /// Atomically install a completed exec image, then release the old one.
    pub fn install_exec(
        &self,
        pagetable: PageTable,
        sz: u64,
        entry: u64,
        sp: u64,
        argv: u64,
        name: [u8; 16],
    ) {
        let mut private = self.private_guard();
        let old_sz = private.sz;
        let old = private
            .pagetable
            .replace(pagetable)
            .expect("exec: old pagetable");
        private.sz = sz;
        private.name = name;
        private.trapframe().set_exec(entry, sp, argv);
        uvm::free_proc_table(old, old_sz);
    }
}

/// A process name for display: the bytes up to the first nul
/// (printk's `%s` over `p->name`).
pub struct ProcName([u8; 16]);

impl core::fmt::Display for ProcName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        as_str(&self.0[..end]).fmt(f)
    }
}

/// Interpret name bytes as `&str`, escaping invalid UTF-8 conservatively
/// (a torn diagnostic read may produce garbage).
fn as_str(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("???")
}

/// Read a slot's private fields.
///
/// # Safety
///
/// Caller must hold the slot's `private_borrowed` guard, or hold the
/// slot's `shared` lock and prove its state is not `Running`. A shared
/// lock alone never permits access to a running process's private fields.
unsafe fn private(p: &Proc) -> &ProcPrivate {
    // SAFETY: caller contract above.
    unsafe { &*private_ptr(p) }
}

/// Raw access to a slot's interior-mutable private cell. Turning this into
/// a reference requires either `PrivateGuard` or the lifecycle lock/state
/// precondition documented by [`private`].
fn private_ptr(p: &Proc) -> *mut ProcPrivate {
    p.private.get()
}

/// Read a slot's parent (under [`WAIT_LOCK`]).
fn parent_slot(slot: usize) -> Option<usize> {
    // SAFETY: WAIT_LOCK held by the caller serializes every writer, so
    // the word is never observed mid-write.
    let raw = unsafe { core::ptr::read(PROCS[slot].parent.get()) };
    (raw != PARENT_NONE).then_some(raw)
}

/// Set a slot's parent (under [`WAIT_LOCK`]).
fn set_parent_slot(slot: usize, parent: Option<usize>) {
    // SAFETY: as `parent_slot`.
    unsafe {
        core::ptr::write(PROCS[slot].parent.get(), parent.unwrap_or(PARENT_NONE));
    }
}

/// A slot's address, as a sleep channel.
fn chan_of(slot: usize) -> usize {
    &PROCS[slot] as *const Proc as usize
}

/// Allocate a fresh process id (`allocpid`, proc.c:92-103).
fn allocpid() -> i32 {
    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

/// Look in the table for an `Unused` slot; initialize the state needed
/// to run in the kernel, and return the slot with its `shared` lock
/// held (`allocproc`, proc.c:105-150). `None` when the table is full or
/// a memory allocation fails.
fn allocproc() -> Option<(usize, SpinGuard<'static, ProcShared>)> {
    for (slot, proc) in PROCS.iter().enumerate() {
        let mut shared = proc.shared.lock();
        if shared.state != ProcState::Unused {
            drop(shared);
            continue;
        }
        shared.pid = allocpid();
        shared.state = ProcState::Used;

        // Allocate a trapframe page (proc.c:128-133).
        let Some(frame) = kalloc::alloc() else {
            freeproc(slot, &mut shared);
            return None;
        };
        // SAFETY: we hold this slot's shared lock with state Used —
        // the alloc path's sanctioned private access.
        let private = unsafe { &mut *private_ptr(proc) };
        private.trapframe_page = Some(frame);

        // An empty user page table with trampoline and trapframe
        // mappings (proc.c:135-141, 172-205).
        let pagetable = private
            .trapframe_page
            .as_ref()
            .map(|frame| proc_pagetable(slot, frame.addr()))
            .unwrap_or(None);
        if pagetable.is_none() {
            freeproc(slot, &mut shared);
            return None;
        }
        private.pagetable = pagetable;

        // Set up the context to start executing at forkret, which
        // returns to user space (proc.c:143-147).
        let forkret_entry: extern "C" fn() = forkret;
        private.context =
            Context::new(forkret_entry as usize as u64, ProcPrivate::kstack_top(slot));

        return Some((slot, shared));
    }
    None
}

/// Create a user page table with no user memory, but with trampoline
/// and trapframe pages (`proc_pagetable`, proc.c:172-205).
fn proc_pagetable(slot: usize, trapframe_pa: PhysAddr) -> Option<PageTable> {
    let mut pt = PageTable::new()?;
    arch::prepare_user_table(&mut pt, slot).ok()?;

    // Map the trampoline code at the highest user virtual address; only
    // the supervisor uses it, so no PTE_U (proc.c:185-193).
    pt.map_range(
        VirtAddr(TRAMPOLINE.0),
        PhysAddr(arch::trampoline_addr()),
        PAGE_SIZE as u64,
        Perm::R | Perm::X,
    )
    .ok()?;

    // Map the trapframe page just below the trampoline (proc.c:195-202).
    if pt
        .map_range(
            VirtAddr(TRAPFRAME.0),
            trapframe_pa,
            PAGE_SIZE as u64,
            Perm::R | Perm::W,
        )
        .is_err()
    {
        // Unmap the trampoline so the Drop-time freewalk finds no leaf,
        // then free the table pages (proc.c:199-200).
        let _ = pt.take_leaf(TRAMPOLINE.0);
        return None;
    }
    Some(pt)
}

/// Free a process structure and the data hanging from it, including
/// user pages (`freeproc`, proc.c:152-171). The slot's `shared` lock
/// must be held.
fn freeproc(slot: usize, shared: &mut SpinGuard<'_, ProcShared>) {
    // SAFETY: caller holds the slot's shared lock — freeproc's
    // precondition (proc.c:154).
    let private = unsafe { &mut *private_ptr(&PROCS[slot]) };
    // Dropping the frame returns the page to the allocator (kfree,
    // proc.c:158-159).
    drop(private.trapframe_page.take());
    if let Some(pt) = private.pagetable.take() {
        uvm::free_proc_table(pt, private.sz);
    }
    private.sz = 0;
    private.name = [0; 16];
    shared.chan = 0;
    shared.killed = false;
    shared.xstate = 0;
    shared.pid = 0;
    shared.state = ProcState::Unused;
}

/// Per-hart process scheduler (`scheduler`, proc.c:421-470): choose a
/// `Runnable` process, `swtch` into it, and repeat forever.
pub fn scheduler() -> ! {
    let c = cpu::current();
    c.set_current_slot(None);
    loop {
        // The most recent process may have had interrupts off; enable
        // them to avoid deadlock if all processes are waiting, then
        // back off to avoid a race between an interrupt and wfi
        // (proc.c:436-441).
        arch::intr_on();
        arch::intr_off();

        let mut found = false;
        for (slot, proc) in PROCS.iter().enumerate() {
            let mut shared = proc.shared.lock();
            if shared.state != ProcState::Runnable {
                drop(shared);
                continue;
            }
            // Switch to the chosen process. It is the process's job to
            // release its lock and then reacquire it before jumping
            // back to us (proc.c:447-453) — so the guard stays held
            // across the switch, forgotten here and released raw below
            // when the process yields the hart back.
            shared.state = ProcState::Running;
            c.set_current_slot(Some(slot));
            mem::forget(shared);
            // SAFETY: interrupts are off (asserted by the whole loop's
            // shape, as in C); the contexts are this hart's scheduler
            // cell and slot `slot`'s private cell, both valid across
            // the switch and distinct.
            unsafe {
                arch::switch(c.scheduler_context(), context_ptr_of(slot));
            }
            // Don't re-enable interrupts on release (proc.c:455-456).
            c.set_intena(false);
            // Process is done running for now; it should have changed
            // its state before coming back (proc.c:458-460).
            c.set_current_slot(None);
            found = true;
            // Release what we acquired before the switch; the process
            // kept it held on this hart the whole time it ran.
            proc.shared.release_raw();
        }
        if !found {
            // Nothing to run; stop until an interrupt (proc.c:465-468).
            arch::wait_for_interrupt();
        }
    }
}

/// A slot's context address, for the scheduler's `swtch`.
fn context_ptr_of(slot: usize) -> *mut Context {
    // SAFETY: only the scheduler (or sched on the running process)
    // reaches this, under the slot's shared lock held — state
    // transitions are serialized.
    unsafe { core::ptr::addr_of_mut!((*private_ptr(&PROCS[slot])).context) }
}

/// Switch to the scheduler (`sched`, proc.c:472-497). Must hold only the
/// current process's `shared` lock and have already changed its state;
/// saves and restores `intena` because it is a property of this kernel
/// thread, not this hart.
fn sched(p: &CurrentProc) {
    let c = cpu::current();
    let shared = p.shared();
    assert!(shared.holding(), "sched p->lock");
    assert!(c.noff() == 1, "sched locks");
    let still_running = shared.with_held(|shared| shared.state == ProcState::Running);
    assert!(!still_running, "sched RUNNING");
    assert!(!arch::intr_get(), "sched interruptible");

    let intena = c.intena();
    // SAFETY: interrupts are off (asserted above); the two contexts are
    // this process's private cell and this hart's scheduler cell, valid
    // and distinct across the switch.
    unsafe {
        arch::switch(p.context_ptr(), c.scheduler_context());
    }
    cpu::current().set_intena(intena);
}

/// Give up the hart for one scheduling round (`yield`, proc.c:499-508).
pub fn yield_now() {
    let p = my_proc().expect("yield: no current proc");
    let mut shared = p.shared().lock();
    shared.state = ProcState::Runnable;
    // The guard must survive the switch: the scheduler on the other side
    // releases it (see `scheduler`), and the scheduler that resumes this
    // process re-acquires it before swtching back.
    mem::forget(shared);
    sched(&p);
    // Back from the scheduler, which set this process Running before
    // switching back into it; release the lock it re-acquired for us
    // (proc.c:506-507).
    p.shared().release_raw();
}

/// Sleep on `chan`, releasing the condition lock `guard` for the duration
/// and re-acquiring it before returning a fresh guard (upstream xv6's
/// `sleep(chan, lk)`; this reference splits it into `sleep_prepare` +
/// `sleep`, proc.c:548-573). The caller must hold `guard`.
///
/// The lost-wakeup rule is the ordering below: `p.shared` is acquired
/// BEFORE the condition lock is released, so a `wakeup` that runs
/// between the two either sees the process still holding the condition
/// lock (and waits) or finds `chan` already set under `p.shared`.
pub fn sleep<T: Send>(chan: usize, guard: SpinGuard<'_, T>) -> SpinGuard<'_, T> {
    let lk = guard.handoff();
    let p = my_proc().expect("sleep: no current proc");
    assert!(chan != 0, "sleep: zero chan");

    let mut shared = p.shared().lock();
    // Must acquire p.shared in order to change state and call sched;
    // once we hold it, no wakeup can be missed (proc.c:558-570).
    lk.release_raw();
    shared.chan = chan;
    shared.state = ProcState::Sleeping;
    mem::forget(shared);
    sched(&p);

    // Tidy up and re-acquire the original lock (proc.c:571-573): the
    // scheduler that resumed us holds p.shared on this hart.
    p.shared().with_held(|shared| shared.chan = 0);
    p.shared().release_raw();
    lk.lock()
}

/// Register the current process as waiting for wakeups on `chan`
/// (`sleep_prepare`, proc.c:546-557). A wakeup between this call and
/// [`sleep_commit`] clears the channel, so the process will not sleep.
pub fn sleep_prepare(chan: usize) {
    let p = my_proc().expect("sleep_prepare: no current proc");
    let mut shared = p.shared().lock();
    assert!(chan != 0, "sleep_prepare: zero chan");
    shared.chan = chan;
}

/// Sleep after [`sleep_prepare`] unless a wakeup has already cleared the
/// registered channel (`sleep`, proc.c:559-573).
pub fn sleep_commit() {
    let p = my_proc().expect("sleep_commit: no current proc");
    let mut shared = p.shared().lock();
    if shared.chan == 0 {
        return;
    }

    shared.state = ProcState::Sleeping;
    mem::forget(shared);
    sched(&p);

    // The scheduler that resumed us holds p.shared on this hart.
    p.shared().release_raw();
}

/// Wake all processes waiting on `chan` (`wakeup`, proc.c:575-596).
/// Clearing `chan` also signals a process that has prepared but not yet
/// committed its sleep; an already sleeping process becomes runnable.
pub fn wakeup(chan: usize) {
    for proc in &PROCS {
        let mut shared = proc.shared.lock();
        if shared.chan == chan {
            shared.chan = 0;
            if shared.state == ProcState::Sleeping {
                shared.state = ProcState::Runnable;
            }
        }
    }
}

/// Create a new process copying the parent (`kfork`, proc.c:256-305):
/// user memory, trapframe (with the child's return value 0), and name.
/// Open files and cwd join with the file table (M5). Returns the child
/// pid.
pub fn fork() -> Result<usize, Err> {
    let p = my_proc().expect("fork: no current proc");
    let Some((nslot, mut nshared)) = allocproc() else {
        return Err(Err::NoMem);
    };

    // Copy user memory from parent to child (proc.c:270-282).
    // SAFETY: the child's shared lock is held with state Used — the
    // fork path's sanctioned private access.
    let nprivate = unsafe { &mut *private_ptr(&PROCS[nslot]) };
    let parent_size = {
        let memory = p.user_memory();
        let pagetable = nprivate.pagetable.as_mut().expect("child pagetable");
        if uvm::copy(memory.pagetable(), pagetable, memory.size()).is_err() {
            freeproc(nslot, &mut nshared);
            return Err(Err::NoMem);
        }
        memory.size()
    };
    nprivate.sz = parent_size;

    // Copy saved user registers; cause fork to return 0 in the child.
    *nprivate.trapframe() = *p.trapframe();
    nprivate.trapframe().set_ret(0);

    nprivate.cwd = p.cwd();
    nprivate.ofile = core::array::from_fn(|fd| p.file(fd));
    // Increment reference counts on open file descriptors (proc.c:285-
    // 288): the ofile array and cwd join with the file table (M5).

    nprivate.name = p.name_bytes();
    let pid = nshared.pid;
    drop(nshared);

    let wl = WAIT_LOCK.lock();
    set_parent_slot(nslot, Some(p.slot()));
    drop(wl);

    let mut nshared = PROCS[nslot].shared.lock();
    nshared.state = ProcState::Runnable;
    drop(nshared);

    Ok(pid as usize)
}

/// Pass a dying process's children to init (`reparent`, proc.c:307-320).
/// Caller must hold [`WAIT_LOCK`].
fn reparent(slot: usize, _wl: &SpinGuard<'_, ()>) {
    let init = INITPROC.load(Ordering::Relaxed);
    for child in 0..NPROC {
        if parent_slot(child) == Some(slot) {
            set_parent_slot(child, Some(init));
            wakeup(chan_of(init));
        }
    }
}

/// Exit the current process; does not return (`kexit`, proc.c:322-365).
/// An exited process stays zombie until its parent calls [`wait`].
pub fn exit(status: i32) -> ! {
    let p = my_proc().expect("exit: no current proc");
    assert!(p.slot() != INITPROC.load(Ordering::Relaxed), "init exiting");
    let cwd = p.cwd();
    p.set_cwd(None);
    if let Some(cwd) = cwd {
        let operation = crate::fs::log::begin_op();
        drop(cwd);
        drop(operation);
    }

    for fd in 0..NOFILE {
        drop(p.replace_file(fd, None));
    }
    // Close all open files and the cwd (proc.c:333-345): joins with the
    // file table (M5); there is nothing to close before then.

    let wl = WAIT_LOCK.lock();

    // Give any children to init, and wake a parent sleeping in wait
    // (proc.c:349-353).
    reparent(p.slot(), &wl);
    if let Some(parent) = parent_slot(p.slot()) {
        wakeup(chan_of(parent));
    }

    let mut shared = p.shared().lock();
    shared.xstate = status;
    shared.state = ProcState::Zombie;
    drop(wl);

    // Jump into the scheduler, never to return (proc.c:362-363).
    mem::forget(shared);
    sched(&p);
    panic!("zombie exit");
}

/// Wait for a child process to exit and return its pid (`kwait`,
/// proc.c:367-419). `status_addr != 0` receives the exit status via
/// copyout. Err when this process has no children or was killed.
pub fn wait(status_addr: u64) -> Result<usize, Err> {
    let p = my_proc().expect("wait: no current proc");
    let mut wl = WAIT_LOCK.lock();
    loop {
        // Scan the table for exited children (proc.c:380-405).
        let mut havekids = false;
        for (slot, proc) in PROCS.iter().enumerate() {
            if parent_slot(slot) != Some(p.slot()) {
                continue;
            }
            // Make sure the child isn't still in exit or swtch.
            let mut shared = proc.shared.lock();
            havekids = true;
            if shared.state == ProcState::Zombie {
                let pid = shared.pid;
                if status_addr != 0 {
                    let xstate = shared.xstate.to_le_bytes();
                    let mut memory = p.user_memory();
                    let size = memory.size();
                    if uvm::copy_out(memory.pagetable_mut(), size, status_addr, &xstate).is_err() {
                        return Err(Err::BadArg);
                    }
                }
                set_parent_slot(slot, None);
                freeproc(slot, &mut shared);
                drop(shared);
                drop(wl);
                return Ok(pid as usize);
            }
            drop(shared);
        }

        // No point waiting if we don't have any children (proc.c:407-411).
        if !havekids || p.killed() {
            drop(wl);
            return Err(Err::NoEnt);
        }

        // Wait for a child to exit, on our own address, holding
        // wait_lock across the sleep (proc.c:413-417).
        wl = sleep(p.chan_addr(), wl);
    }
}

/// Kill the process with the given pid (`kkill`, proc.c:598-620). The
/// victim won't exit until it tries to return to user space (see
/// `usertrap`).
pub fn kill(pid: i32) -> Result<(), Err> {
    for proc in &PROCS {
        let mut shared = proc.shared.lock();
        if shared.pid == pid {
            shared.killed = true;
            if shared.state == ProcState::Sleeping {
                // Wake the process from sleep (proc.c:610-613).
                shared.state = ProcState::Runnable;
            }
            return Ok(());
        }
    }
    Err(Err::NoEnt)
}

/// Grow or shrink the current process's user memory by `n` bytes
/// (`growproc`, proc.c:233-254).
pub fn grow(n: i64) -> Result<(), Err> {
    let p = my_proc().expect("grow: no current proc");
    let mut memory = p.user_memory();
    let sz = memory.size();
    if n > 0 {
        let n = n as u64;
        if sz + n > TRAPFRAME.0 {
            return Err(Err::TooBig);
        }
        let newsz = uvm::alloc(memory.pagetable_mut(), sz, sz + n, Perm::W)?;
        memory.set_size(newsz);
    } else if n < 0 {
        let newsz = uvm::dealloc(memory.pagetable_mut(), sz, (sz as i64 + n) as u64);
        memory.set_size(newsz);
    }
    Ok(())
}

/// Copy from either a user or kernel address (`either_copyin`,
/// proc.c:656-669).
pub fn either_copy_in(dst: &mut [u8], user_src: bool, srcva: u64) -> Result<(), Err> {
    if !user_src {
        // SAFETY: as `either_copy_out` — a kernel source the caller
        // owns for `dst.len()` bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(
                srcva as usize as *const u8,
                dst.as_mut_ptr(),
                dst.len(),
            );
        }
        Ok(())
    } else {
        let p = my_proc().expect("either_copy_in: no current proc");
        let mut memory = p.user_memory();
        let size = memory.size();
        uvm::copy_in(memory.pagetable_mut(), size, dst, srcva)
    }
}

/// Copy to either a user or kernel address (`either_copyout`, proc.c).
pub fn either_copy_out(src: &[u8], user_dst: bool, dstva: u64) -> Result<(), Err> {
    if !user_dst {
        // SAFETY: kernel callers provide writable storage for `src.len()`
        // bytes; the regions do not overlap.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dstva as usize as *mut u8, src.len());
        }
        Ok(())
    } else {
        let p = my_proc().expect("either_copy_out: no current proc");
        let mut memory = p.user_memory();
        let size = memory.size();
        uvm::copy_out(memory.pagetable_mut(), size, dstva, src)
    }
}

/// A fork child's first scheduling lands here (`forkret`, proc.c:510-543).
extern "C" fn forkret() {
    let p = my_proc().expect("forkret: no current proc");
    // Still holding p.shared from scheduler (proc.c:519-520).
    p.shared().release_raw();

    if !FS_INITIALIZED.swap(true, Ordering::AcqRel) {
        crate::fs::init();
        p.set_cwd(Some(crate::fs::inode::get(abi::ROOTDEV, abi::ROOTINO)));
        let argc =
            crate::exec::exec(b"/init", &[b"/init"]).unwrap_or_else(|_| panic!("exec /init"));
        p.trapframe().set_entry_arg(argc as u64);
    }

    crate::trap::usertrapret(&p);
}

/// Print a process listing (`procdump`, proc.c:671-701) — the ^P
/// handler. Each slot's shared fields are read under its lock. Private
/// fields are read only when that state proves the process is not running;
/// a running process gets a fixed marker instead, because its private
/// fields may be changing without the shared lock.
pub fn procdump() {
    println!();
    for proc in &PROCS {
        let shared = proc.shared.lock();
        if shared.state == ProcState::Unused {
            continue;
        }
        let state = match shared.state {
            ProcState::Unused => "unused",
            ProcState::Used => "used",
            ProcState::Sleeping => "sleep ",
            ProcState::Runnable => "runble",
            ProcState::Running => "run   ",
            ProcState::Zombie => "zombie",
        };
        if shared.state == ProcState::Running {
            println!("{} {} <running>", shared.pid, state);
            continue;
        }
        // SAFETY: the shared lock is held and the state was proved not to
        // be Running, so no running process can mutate its private fields.
        let name = unsafe { private(proc) }.name;
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        println!("{} {} {}", shared.pid, state, as_str(&name[..end]));
    }
}

/// Allocate the first process. Its first `forkret` initializes the file
/// system and execs `/init`; no temporary M4 user image remains.
pub fn user_init() {
    let (slot, mut shared) = allocproc().expect("userinit: allocproc");
    INITPROC.store(slot, Ordering::Release);
    shared.state = ProcState::Runnable;
}

pub fn cwd() -> Option<Inode> {
    my_proc().and_then(|process| process.cwd())
}
