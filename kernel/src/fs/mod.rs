//! Buffer cache, log, and xv6 on-disk filesystem.

pub mod bcache;
pub mod file;
pub mod inode;
pub mod log;

use crate::sync::SpinLock;
use abi::{FSMAGIC, ROOTDEV, Superblock};

static SUPERBLOCK: SpinLock<Superblock> = SpinLock::new(Superblock {
    magic: 0,
    size: 0,
    nblocks: 0,
    ninodes: 0,
    nlog: 0,
    logstart: 0,
    inodestart: 0,
    bmapstart: 0,
});

pub(crate) fn superblock() -> Superblock {
    *SUPERBLOCK.lock()
}

pub fn init() {
    let buffer = bcache::bread(ROOTDEV, 1);
    let sb = Superblock::decode(buffer.data()).expect("invalid superblock encoding");
    assert_eq!(sb.magic, FSMAGIC, "invalid file system");
    *SUPERBLOCK.lock() = sb;
    drop(buffer);
    log::init(ROOTDEV, sb);
}
