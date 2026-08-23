//! Interim fail-fast bodies for the alphabetic N-Z usertests shard.

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
    openiput,
    opentest,
    outofinodes,
    pipe1,
    preempt,
    reparent,
    reparent2,
    rmdot,
    sharedfd,
    subdir,
    truncate1,
    truncate2,
    truncate3,
    twochildren,
    unlinkcwd,
    unlinkread,
    writebig,
    writetest,
);
