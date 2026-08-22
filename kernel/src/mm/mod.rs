//! Kernel memory management: physical frames, the frame allocator, and
//! the kernel address space (`kernel/kalloc.c`, `kernel/vm.c`).

pub mod addr;
pub mod frame;
pub mod kalloc;
pub mod kernel_map;
pub mod layout;

/// Boot smoke test: one allocate / print / free round trip through the
/// frame allocator, proving both `alloc` and the `Drop` free path once
/// paging is on. The body runs only in debug builds.
pub fn selftest() {
    if cfg!(debug_assertions) {
        let frame = kalloc::alloc().expect("kalloc: no frames at boot");
        println!("kalloc selftest: frame {:#x}", frame.addr().0);
        drop(frame);
    }
}
