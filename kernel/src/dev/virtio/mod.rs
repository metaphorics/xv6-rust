//! Architecture-neutral virtio block core.

pub mod blk;
mod queue;

use crate::arch::virtio_transport as transport;
