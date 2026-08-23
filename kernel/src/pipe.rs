//! Byte pipes backed by a fixed safe pool (`kernel/pipe.c`).

use crate::err::Err;
use crate::params::NFILE;
use crate::proc;
use crate::sync::SpinLock;

const PIPE_SIZE: usize = 512;

#[derive(Clone, Copy)]
struct Pipe {
    data: [u8; PIPE_SIZE],
    read_pos: usize,
    write_pos: usize,
    read_open: bool,
    write_open: bool,
}

impl Pipe {
    const EMPTY: Self = Self {
        data: [0; PIPE_SIZE],
        read_pos: 0,
        write_pos: 0,
        read_open: false,
        write_open: false,
    };

    const OPEN: Self = Self {
        read_open: true,
        write_open: true,
        ..Self::EMPTY
    };

    fn used(&self) -> bool {
        self.read_open || self.write_open
    }

    fn buffered(&self) -> usize {
        self.write_pos.wrapping_sub(self.read_pos)
    }
}

static PIPES: SpinLock<[Pipe; NFILE]> = SpinLock::new([Pipe::EMPTY; NFILE]);

#[derive(Clone, Copy)]
pub struct PipeEnd {
    index: usize,
    readable: bool,
}

pub fn alloc() -> Option<(PipeEnd, PipeEnd)> {
    let mut pipes = PIPES.lock();
    let index = pipes.iter().position(|pipe| !pipe.used())?;
    pipes[index] = Pipe::OPEN;
    Some((
        PipeEnd {
            index,
            readable: true,
        },
        PipeEnd {
            index,
            readable: false,
        },
    ))
}

impl PipeEnd {
    pub fn read(self, user_dst: bool, dst: u64, n: usize) -> Result<usize, Err> {
        debug_assert!(self.readable);
        let mut pipes = PIPES.lock();
        while pipes[self.index].buffered() == 0 && pipes[self.index].write_open {
            if proc::my_proc().is_some_and(|process| process.killed()) {
                return Err(Err::BadArg);
            }
            pipes = proc::sleep(read_chan(self.index), pipes);
        }

        let mut done = 0;
        let mut copy_failed = false;
        while done < n && pipes[self.index].buffered() != 0 {
            let at = pipes[self.index].read_pos % PIPE_SIZE;
            let byte = [pipes[self.index].data[at]];
            if proc::either_copy_out(&byte, user_dst, dst + done as u64).is_err() {
                copy_failed = true;
                break;
            }
            pipes[self.index].read_pos = pipes[self.index].read_pos.wrapping_add(1);
            done += 1;
        }
        if pipes[self.index].read_pos == pipes[self.index].write_pos {
            pipes[self.index].read_pos = 0;
            pipes[self.index].write_pos = 0;
        }
        drop(pipes);
        proc::wakeup(write_chan(self.index));
        if copy_failed && done == 0 {
            Err(Err::BadArg)
        } else {
            Ok(done)
        }
    }

    pub fn write(self, user_src: bool, src: u64, n: usize) -> Result<usize, Err> {
        debug_assert!(!self.readable);
        let mut pipes = PIPES.lock();
        let mut done = 0;
        let mut copy_failed = false;
        while done < n {
            if !pipes[self.index].read_open
                || proc::my_proc().is_some_and(|process| process.killed())
            {
                return Err(Err::BadArg);
            }
            while pipes[self.index].buffered() == PIPE_SIZE {
                proc::wakeup(read_chan(self.index));
                pipes = proc::sleep(write_chan(self.index), pipes);
                if !pipes[self.index].read_open
                    || proc::my_proc().is_some_and(|process| process.killed())
                {
                    return Err(Err::BadArg);
                }
            }
            let mut byte = [0];
            if proc::either_copy_in(&mut byte, user_src, src + done as u64).is_err() {
                copy_failed = true;
                break;
            }
            let at = pipes[self.index].write_pos % PIPE_SIZE;
            pipes[self.index].data[at] = byte[0];
            pipes[self.index].write_pos = pipes[self.index].write_pos.wrapping_add(1);
            done += 1;
        }
        drop(pipes);
        proc::wakeup(read_chan(self.index));
        if copy_failed && done == 0 {
            Err(Err::BadArg)
        } else {
            Ok(done)
        }
    }

    pub fn close(self) {
        let mut pipes = PIPES.lock();
        let pipe = &mut pipes[self.index];
        if self.readable {
            assert!(pipe.read_open, "pipe read close");
            pipe.read_open = false;
            proc::wakeup(write_chan(self.index));
        } else {
            assert!(pipe.write_open, "pipe write close");
            pipe.write_open = false;
            proc::wakeup(read_chan(self.index));
        }
        if !pipe.used() {
            *pipe = Pipe::EMPTY;
        }
    }
}

fn read_chan(index: usize) -> usize {
    PIPES.chan() + index * 2 + 1
}

fn write_chan(index: usize) -> usize {
    PIPES.chan() + index * 2 + 2
}
