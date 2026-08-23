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
    selftest();
}

fn selftest() {
    use abi::{BSIZE, FileType, ROOTINO};
    use file::FileHandle;

    let superblock_buffer = bcache::bread(ROOTDEV, 1);
    assert_eq!(superblock_buffer.dev(), ROOTDEV);
    assert_eq!(superblock_buffer.blockno(), 1);
    drop(superblock_buffer);

    let operation = log::begin_op();
    let root = inode::get(ROOTDEV, ROOTINO);
    let data_inode = inode::alloc(ROOTDEV, FileType::File).expect("fs selftest inode");
    {
        let mut metadata = data_inode.lock();
        metadata.set_nlink(1);
        metadata.update();
    }
    {
        let mut directory = root.lock();
        assert!(directory.dir_link(b"m5check", data_inode.inum()));
    }

    let empty_inode = inode::alloc(ROOTDEV, FileType::File).expect("fs selftest empty inode");
    {
        let mut metadata = empty_inode.lock();
        metadata.set_nlink(1);
        assert_eq!(metadata.write_at(b"discard", 0), 7);
        metadata.truncate();
        assert_eq!(metadata.size(), 0);
    }
    {
        let mut directory = root.lock();
        assert!(directory.dir_link(b"m5empty", empty_inode.inum()));
    }

    let device_inode = inode::alloc(ROOTDEV, FileType::Device).expect("fs selftest device inode");
    {
        let mut metadata = device_inode.lock();
        metadata.set_nlink(1);
        metadata.set_device(1, 0);
        metadata.update();
        assert_eq!(metadata.major(), 1);
    }
    {
        let mut directory = root.lock();
        assert!(directory.dir_link(b"m5dev", device_inode.inum()));
    }
    drop(device_inode);
    drop(empty_inode);
    drop(root);
    drop(operation);

    let file = FileHandle::inode(data_inode, true, true).expect("fs selftest file slot");
    let mut payload = [0; BSIZE + 37];
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(17).wrapping_add(3);
    }
    assert_eq!(
        file.write(false, payload.as_ptr() as u64, payload.len()),
        Ok(payload.len())
    );
    assert_eq!(
        file.stat().expect("fs selftest stat").size,
        payload.len() as u64
    );
    drop(file);

    let operation = log::begin_op();
    let reopened = inode::namei(b"/m5check").expect("fs selftest namei");
    let (parent, name) = inode::nameiparent(b"/m5check").expect("fs selftest parent");
    assert_eq!(&name[..7], b"m5check");
    assert_eq!(parent.inum(), ROOTINO);
    drop(parent);
    drop(operation);

    let file = FileHandle::inode(reopened, true, false).expect("fs selftest reopen slot");
    let mut readback = [0; BSIZE + 37];
    assert_eq!(
        file.read(false, readback.as_mut_ptr() as u64, readback.len()),
        Ok(readback.len())
    );
    assert_eq!(readback, payload);
    drop(file);

    let device = FileHandle::device(1, true, true).expect("fs selftest device slot");
    assert_eq!(
        device.stat().expect("fs selftest device stat").r#type,
        FileType::Device as i16
    );
    assert_eq!(device.write(false, payload.as_ptr() as u64, 0), Ok(0));
    drop(device);

    println!("fs selftest: all layers passed");
}
