//! Buffer cache, log, and xv6 on-disk filesystem.

pub mod bcache;
pub mod log;

use abi::{FSMAGIC, ROOTDEV, Superblock};

pub fn init() {
    let buffer = bcache::bread(ROOTDEV, 1);
    let sb = Superblock::decode(buffer.data()).expect("invalid superblock encoding");
    assert_eq!(sb.magic, FSMAGIC, "invalid file system");
    drop(buffer);
    log::init(ROOTDEV, sb);
}
