//! The kernel-wide error type — one enum, no per-module error zoo.
//!
//! Syscall handlers return `Result<usize, Err>`; the dispatch layer
//! maps every `Err` to the `-1` the C handlers return by hand.

/// Kernel error. Grows as subsystems land; the current set is what the
/// process and memory layers can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Err {
    /// Argument rejected (bad fd, bad pointer, bad syscall number).
    BadArg,
    /// Out of memory for the request.
    NoMem,
    /// No such entity (pid, child).
    NoEnt,
    /// Requested growth past the user address space.
    TooBig,
}
