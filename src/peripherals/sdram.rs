use log::{trace, warn};
use crate::{device::UnicornContext, log_unsupported_read, log_unsupported_write, peripherals::common::{mmio_get_store_only, mmio_set_store_only}};

pub const BASE: u64 = 0xB0003000;
pub const SIZE: usize = 0x1000;

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }
    mmio_get_store_only(uc, addr)
}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    if size != 4 {
        log_unsupported_write!(addr, size, value);
        return;
    }
    trace!("0x{:08x} <= 0x{:08x}", BASE + addr, value);
    mmio_set_store_only(uc, addr, value);
}
