use log::error;
use unicorn_engine::MemType;

use crate::device::UnicornContext;

pub fn unmapped_access(_uc: &mut UnicornContext, access_type: MemType, addr: u64, size: usize, value: i64) -> bool {
    error!("exception: {access_type:?} of {size} bytes at 0x{addr:08x}, value 0x{value:08x}.");
    false
}
