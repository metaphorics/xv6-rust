#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use ustd::abi::fcntl::O_RDONLY;
use ustd::abi::{DIRSIZ, Dirent, FileType};

ustd::entry!(main);

fn main(args: &[&[u8]]) -> i32 {
    if args.len() == 1 {
        return list(b".");
    }
    let mut status = 0;
    for path in &args[1..] {
        status |= list(path);
    }
    status
}

fn list(path: &[u8]) -> i32 {
    let fd = ustd::open(path, O_RDONLY);
    if fd < 0 {
        ustd::println!("ls: cannot open {}", display(path));
        return 1;
    }
    let Ok(stat) = ustd::fstat(fd) else {
        ustd::println!("ls: cannot stat {}", display(path));
        let _ = ustd::close(fd);
        return 1;
    };
    if stat.r#type != FileType::Dir as i16 {
        print_entry(path, stat.r#type, stat.ino, stat.size);
        let _ = ustd::close(fd);
        return 0;
    }

    let mut raw = [0; Dirent::ENCODED_LEN];
    loop {
        let n = ustd::read(fd, &mut raw);
        if n == 0 {
            break;
        }
        if n != raw.len() as isize {
            ustd::println!("ls: directory read error");
            let _ = ustd::close(fd);
            return 1;
        }
        let Some(entry) = Dirent::decode(&raw) else {
            continue;
        };
        if entry.inum == 0 {
            continue;
        }
        let name_len = entry
            .name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(DIRSIZ);
        let name = &entry.name[..name_len];
        let child = child_path(path, name);
        let child_fd = ustd::open(&child, O_RDONLY);
        if child_fd < 0 {
            continue;
        }
        if let Ok(child_stat) = ustd::fstat(child_fd) {
            print_entry(name, child_stat.r#type, child_stat.ino, child_stat.size);
        }
        let _ = ustd::close(child_fd);
    }
    let _ = ustd::close(fd);
    0
}

fn child_path(parent: &[u8], name: &[u8]) -> Vec<u8> {
    let mut path = Vec::with_capacity(parent.len() + name.len() + 1);
    path.extend_from_slice(parent);
    if !parent.is_empty() && parent.last() != Some(&b'/') {
        path.push(b'/');
    }
    path.extend_from_slice(name);
    path
}

fn print_entry(name: &[u8], kind: i16, ino: u32, size: u64) {
    ustd::println!("{} {kind} {ino} {size}", display(name));
}

fn display(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
