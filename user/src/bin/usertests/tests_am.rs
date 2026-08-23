//! Interim fail-fast bodies for the alphabetic A-M usertests shard.

macro_rules! unported {
    ($($test:ident),+ $(,)?) => {
        $(
            pub fn $test(name: &[u8]) {
                crate::unported(name)
            }
        )+
    };
}

unported!(
    badwrite,
    bigdir,
    bigfile,
    bigwrite,
    concreate,
    createdelete,
    createtest,
    dirfile,
    dirtest,
    diskfull,
    execout,
    exectest,
    exitiput,
    exitwait,
    forkfork,
    forkforkfork,
    forktest,
    fourfiles,
    fourteen,
    iput,
    iref,
    killstatus,
    linktest,
    linkunlink,
    manywrites,
    mem,
);
