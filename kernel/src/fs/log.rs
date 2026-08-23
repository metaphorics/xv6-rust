//! Physical write-ahead log (`kernel/log.c`).

use abi::{LOGBLOCKS, LogHeader, MAXOPBLOCKS, Superblock};

use super::bcache::{self, BufGuard};
use crate::proc;
use crate::sync::SpinLock;

#[derive(Clone, Copy)]
struct Log {
    start: u32,
    size: u32,
    outstanding: u32,
    committing: bool,
    commits: u64,
    dev: u32,
    header: LogHeader,
}

impl Log {
    const fn new() -> Self {
        Self {
            start: 0,
            size: 0,
            outstanding: 0,
            committing: false,
            dev: 0,
            commits: 0,
            header: LogHeader {
                n: 0,
                block: [0; LOGBLOCKS],
            },
        }
    }
}

static LOG: SpinLock<Log> = SpinLock::new(Log::new());

pub fn init(dev: u32, sb: Superblock) {
    assert!(core::mem::size_of::<LogHeader>() < abi::BSIZE, "log header");
    {
        let mut log = LOG.lock();
        log.start = sb.logstart;
        log.size = sb.nlog;
        log.dev = dev;
        log.outstanding = 0;
        log.commits = 0;
        log.committing = false;
        log.header = LogHeader::default();
    }
    read_head();
    install_trans(true);
    {
        LOG.lock().header.n = 0;
    }
    write_head();
}

/// Reserve log space for one filesystem operation (`begin_op`).
pub fn begin_op() -> OpGuard {
    let mut log = LOG.lock();
    loop {
        let reserved = (log.outstanding as usize + 1) * MAXOPBLOCKS;
        if !log.committing && log.header.n as usize + reserved <= LOGBLOCKS {
            log.outstanding += 1;
            return OpGuard { active: true };
        }
        log = proc::sleep(LOG.chan(), log);
    }
}

/// Caller-owned transaction token. Dropping performs `end_op`; inode Drop
/// never creates a nested transaction.
pub struct OpGuard {
    active: bool,
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        end_op();
    }
}

fn end_op() {
    let mut log = LOG.lock();
    assert!(log.outstanding != 0, "end_op");
    log.outstanding -= 1;
    if log.outstanding != 0 {
        drop(log);
        proc::wakeup(LOG.chan());
        return;
    }
    log.committing = true;
    drop(log);

    commit();

    let mut log = LOG.lock();
    log.committing = false;
    log.commits = log.commits.wrapping_add(1);
    drop(log);
    proc::wakeup(LOG.chan());
}

/// Wait until all operations active at the call boundary are committed.
pub fn sync() {
    let mut log = LOG.lock();
    if !log.committing && log.outstanding == 0 {
        return;
    }
    let target = log.commits.wrapping_add(1);
    while log.commits < target {
        log = proc::sleep(LOG.chan(), log);
    }
}

/// Record a dirty buffer in the current transaction (`log_write`).
pub fn write(buffer: &BufGuard) {
    let mut log = LOG.lock();
    assert!(log.outstanding != 0, "log_write outside transaction");
    let n = log.header.n as usize;
    if let Some(index) = log.header.block[..n]
        .iter()
        .position(|block| *block == buffer.blockno())
    {
        log.header.block[index] = buffer.blockno();
        return;
    }
    assert!(
        n < LOGBLOCKS && n < log.size.saturating_sub(1) as usize,
        "too big a transaction"
    );
    log.header.block[n] = buffer.blockno();
    log.header.n += 1;
    bcache::pin(buffer);
}

fn read_head() {
    let (dev, start) = {
        let log = LOG.lock();
        (log.dev, log.start)
    };
    let buffer = bcache::bread(dev, start);
    let header = LogHeader::decode_block(buffer.data()).expect("invalid log header");
    LOG.lock().header = header;
}

fn write_head() {
    let (dev, start, header) = {
        let log = LOG.lock();
        (log.dev, log.start, log.header)
    };
    let mut buffer = bcache::bread(dev, start);
    buffer.data_mut().copy_from_slice(&header.encode_block());
    buffer.write();
}

/// Copy committed log blocks to their home locations (`install_trans`).
fn install_trans(recovering: bool) {
    let (dev, start, header) = {
        let log = LOG.lock();
        (log.dev, log.start, log.header)
    };
    for tail in 0..header.n as usize {
        let log_buffer = bcache::bread(dev, start + tail as u32 + 1);
        let mut destination = bcache::bread(dev, header.block[tail]);
        destination.data_mut().copy_from_slice(log_buffer.data());
        destination.write();
        drop(log_buffer);
        if !recovering {
            bcache::unpin(&destination);
        }
    }
}

/// Four-phase commit: log data, durable header (commit point), install,
/// clear the header (`commit`, log.c:193-203).
fn commit() {
    let n = LOG.lock().header.n;
    if n == 0 {
        return;
    }
    write_log();
    write_head();
    install_trans(false);
    LOG.lock().header.n = 0;
    write_head();
}

fn write_log() {
    let (dev, start, header) = {
        let log = LOG.lock();
        (log.dev, log.start, log.header)
    };
    for tail in 0..header.n as usize {
        let mut destination = bcache::bread(dev, start + tail as u32 + 1);
        let source = bcache::bread(dev, header.block[tail]);
        destination.data_mut().copy_from_slice(source.data());
        destination.write();
    }
}
