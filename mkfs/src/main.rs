#![forbid(unsafe_code)]

//! Host-side xv6 file-system image builder (`mkfs/mkfs.c`).

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use abi::{
    BPB, BSIZE, DIRSIZ, Dinode, Dirent, FSMAGIC, FSSIZE, FileType, IPB, LOGBLOCKS, MAXFILE,
    NDIRECT, NINDIRECT, NINODES, ROOTINO, Superblock, inode_block,
};

fn main() {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let Some(image) = args.next() else {
        eprintln!("Usage: {} fs.img files...", Path::new(&program).display());
        std::process::exit(1);
    };
    let files: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if let Err(err) = build_image(Path::new(&image), &files) {
        eprintln!("mkfs: {err}");
        std::process::exit(1);
    }
}

fn layout() -> Superblock {
    let nbitmap = FSSIZE / BPB + 1;
    let ninodeblocks = NINODES / IPB + 1;
    let nlog = LOGBLOCKS as u32 + 1;
    let nmeta = 2 + nlog + ninodeblocks + nbitmap;
    Superblock {
        magic: FSMAGIC,
        size: FSSIZE,
        nblocks: FSSIZE - nmeta,
        ninodes: NINODES,
        nlog,
        logstart: 2,
        inodestart: 2 + nlog,
        bmapstart: 2 + nlog + ninodeblocks,
    }
}

fn build_image(image: &Path, inputs: &[PathBuf]) -> io::Result<()> {
    let sb = layout();
    let mut disk = Image {
        file: OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(image)?,
        sb,
        free_inode: ROOTINO,
        free_block: FSSIZE - sb.nblocks,
    };

    let zero = [0; BSIZE];
    for block in 0..FSSIZE {
        disk.write_block(block, &zero)?;
    }
    let mut superblock = [0; BSIZE];
    superblock[..Superblock::ENCODED_LEN].copy_from_slice(&sb.encode());
    disk.write_block(1, &superblock)?;

    let root = disk.alloc_inode(FileType::Dir)?;
    assert_eq!(root, ROOTINO);
    disk.append(
        root,
        &Dirent::new(root as u16, b".").expect("dot dirent").encode(),
    )?;
    disk.append(
        root,
        &Dirent::new(root as u16, b"..")
            .expect("dot-dot dirent")
            .encode(),
    )?;

    for input in inputs {
        let name = image_name(input)?;
        if name.len() > DIRSIZ {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{}: file name is longer than {DIRSIZ} bytes",
                    input.display()
                ),
            ));
        }
        let mut source = File::open(input)?;
        let inum = disk.alloc_inode(FileType::File)?;
        disk.append(
            root,
            &Dirent::new(inum as u16, name)
                .expect("validated dirent name")
                .encode(),
        )?;
        let mut buf = [0; BSIZE];
        loop {
            let n = source.read(&mut buf)?;
            if n == 0 {
                break;
            }
            disk.append(inum, &buf[..n])?;
        }
    }

    let mut root_inode = disk.read_inode(root)?;
    root_inode.size = (root_inode.size / BSIZE as u32 + 1) * BSIZE as u32;
    disk.write_inode(root, root_inode)?;
    disk.write_bitmap()?;
    disk.file.sync_all()
}

fn image_name(path: &Path) -> io::Result<&[u8]> {
    use std::os::unix::ffi::OsStrExt;

    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: missing file name", path.display()),
        )
    })?;
    let mut bytes = file_name.as_bytes();
    if bytes.first() == Some(&b'_') {
        bytes = &bytes[1..];
    }
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: empty image name", path.display()),
        ));
    }
    Ok(bytes)
}

struct Image {
    file: File,
    sb: Superblock,
    free_inode: u32,
    free_block: u32,
}

impl Image {
    fn write_block(&mut self, block: u32, data: &[u8; BSIZE]) -> io::Result<()> {
        self.file
            .seek(SeekFrom::Start(u64::from(block) * BSIZE as u64))?;
        self.file.write_all(data)
    }

    fn read_block(&mut self, block: u32) -> io::Result<[u8; BSIZE]> {
        let mut data = [0; BSIZE];
        self.file
            .seek(SeekFrom::Start(u64::from(block) * BSIZE as u64))?;
        self.file.read_exact(&mut data)?;
        Ok(data)
    }

    fn alloc_inode(&mut self, kind: FileType) -> io::Result<u32> {
        let inum = self.free_inode;
        self.free_inode += 1;
        if inum >= self.sb.ninodes {
            return Err(io::Error::other("out of inodes"));
        }
        self.write_inode(
            inum,
            Dinode {
                r#type: kind as i16,
                nlink: 1,
                ..Dinode::default()
            },
        )?;
        Ok(inum)
    }

    fn read_inode(&mut self, inum: u32) -> io::Result<Dinode> {
        let block = self.read_block(inode_block(inum, self.sb.inodestart))?;
        let at = inum as usize % IPB as usize * Dinode::ENCODED_LEN;
        Dinode::decode(&block[at..at + Dinode::ENCODED_LEN])
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad dinode"))
    }

    fn write_inode(&mut self, inum: u32, inode: Dinode) -> io::Result<()> {
        let blockno = inode_block(inum, self.sb.inodestart);
        let mut block = self.read_block(blockno)?;
        let at = inum as usize % IPB as usize * Dinode::ENCODED_LEN;
        block[at..at + Dinode::ENCODED_LEN].copy_from_slice(&inode.encode());
        self.write_block(blockno, &block)
    }

    fn alloc_block(&mut self) -> io::Result<u32> {
        if self.free_block >= FSSIZE {
            return Err(io::Error::other("out of blocks"));
        }
        let block = self.free_block;
        self.free_block += 1;
        Ok(block)
    }

    fn append(&mut self, inum: u32, mut src: &[u8]) -> io::Result<()> {
        let mut inode = self.read_inode(inum)?;
        let mut off = inode.size as usize;
        while !src.is_empty() {
            let fbn = off / BSIZE;
            if fbn >= MAXFILE {
                return Err(io::Error::other("file too large"));
            }
            let addr = if fbn < NDIRECT {
                if inode.addrs[fbn] == 0 {
                    inode.addrs[fbn] = self.alloc_block()?;
                }
                inode.addrs[fbn]
            } else {
                if inode.addrs[NDIRECT] == 0 {
                    inode.addrs[NDIRECT] = self.alloc_block()?;
                }
                let indirect_addr = inode.addrs[NDIRECT];
                let mut indirect = self.read_block(indirect_addr)?;
                let index = fbn - NDIRECT;
                debug_assert!(index < NINDIRECT);
                let at = index * 4;
                let mut entry = u32::from_le_bytes(
                    indirect[at..at + 4]
                        .try_into()
                        .expect("four-byte indirect entry"),
                );
                if entry == 0 {
                    entry = self.alloc_block()?;
                    indirect[at..at + 4].copy_from_slice(&entry.to_le_bytes());
                    self.write_block(indirect_addr, &indirect)?;
                }
                entry
            };

            let mut data = self.read_block(addr)?;
            let within = off % BSIZE;
            let n = src.len().min(BSIZE - within);
            data[within..within + n].copy_from_slice(&src[..n]);
            self.write_block(addr, &data)?;
            off += n;
            src = &src[n..];
        }
        inode.size = u32::try_from(off).map_err(|_| io::Error::other("file too large"))?;
        self.write_inode(inum, inode)
    }

    fn write_bitmap(&mut self) -> io::Result<()> {
        let used = self.free_block;
        let blocks = FSSIZE.div_ceil(BPB);
        for bitmap_index in 0..blocks {
            let mut bitmap = [0; BSIZE];
            let base = bitmap_index * BPB;
            let limit = used.min(base + BPB);
            for block in base..limit {
                let bit = block - base;
                bitmap[(bit / 8) as usize] |= 1 << (bit % 8);
            }
            self.write_block(self.sb.bmapstart + bitmap_index, &bitmap)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn image_round_trip_matches_xv6_layout() -> io::Result<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let dir = std::env::temp_dir();
        let image = dir.join(format!("xv6-rust-mkfs-{}-{nonce}.img", std::process::id()));
        let input = dir.join(format!("m5{}.txt", std::process::id() % 100_000));
        let contents = b"xv6 is a re-implementation of Dennis Ritchie's and Ken Thompson's Unix\n";
        std::fs::write(&input, contents)?;
        build_image(&image, std::slice::from_ref(&input))?;

        let result = inspect_image(
            &image,
            input.file_name().expect("input file name"),
            contents,
        );
        let _ = std::fs::remove_file(&image);
        let _ = std::fs::remove_file(&input);
        result
    }

    fn inspect_image(
        image: &Path,
        input_name: &std::ffi::OsStr,
        contents: &[u8],
    ) -> io::Result<()> {
        use std::os::unix::ffi::OsStrExt;

        let mut file = File::open(image)?;
        assert_eq!(file.metadata()?.len(), u64::from(FSSIZE) * BSIZE as u64);
        let mut block = [0; BSIZE];
        file.seek(SeekFrom::Start(BSIZE as u64))?;
        file.read_exact(&mut block)?;
        let sb = Superblock::decode(&block).expect("superblock codec");
        assert_eq!(sb, layout());
        assert_eq!((sb.logstart, sb.inodestart, sb.bmapstart), (2, 33, 46));
        assert_eq!(sb.nblocks, 1_953);

        let root = read_inode_from(&mut file, sb, ROOTINO)?;
        assert_eq!(root.r#type, FileType::Dir as i16);
        assert_eq!(root.size, BSIZE as u32);
        let root_data = read_file_from(&mut file, root)?;
        let (entry_bytes, _) =
            root_data[..3 * Dirent::ENCODED_LEN].as_chunks::<{ Dirent::ENCODED_LEN }>();
        let entries: Vec<Dirent> = entry_bytes
            .iter()
            .map(|bytes| Dirent::decode(bytes).expect("dirent codec"))
            .collect();
        assert_eq!(entries[0], Dirent::new(ROOTINO as u16, b".").expect("dot"));
        assert_eq!(
            entries[1],
            Dirent::new(ROOTINO as u16, b"..").expect("dot-dot")
        );
        assert_eq!(trim_name(&entries[2].name), input_name.as_bytes());

        let file_inode = read_inode_from(&mut file, sb, u32::from(entries[2].inum))?;
        assert_eq!(
            &read_file_from(&mut file, file_inode)?[..contents.len()],
            contents
        );
        Ok(())
    }

    fn read_inode_from(file: &mut File, sb: Superblock, inum: u32) -> io::Result<Dinode> {
        let mut block = [0; BSIZE];
        file.seek(SeekFrom::Start(
            u64::from(inode_block(inum, sb.inodestart)) * BSIZE as u64,
        ))?;
        file.read_exact(&mut block)?;
        let at = inum as usize % IPB as usize * Dinode::ENCODED_LEN;
        Dinode::decode(&block[at..at + Dinode::ENCODED_LEN])
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad dinode"))
    }

    fn read_file_from(file: &mut File, inode: Dinode) -> io::Result<Vec<u8>> {
        let mut result = vec![0; inode.size as usize];
        for (fbn, chunk) in result.chunks_mut(BSIZE).enumerate() {
            let blockno = inode.addrs[fbn];
            file.seek(SeekFrom::Start(u64::from(blockno) * BSIZE as u64))?;
            file.read_exact(chunk)?;
        }
        Ok(result)
    }

    fn trim_name(name: &[u8; DIRSIZ]) -> &[u8] {
        &name[..name.iter().position(|byte| *byte == 0).unwrap_or(DIRSIZ)]
    }
}
