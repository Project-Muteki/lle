use log::warn;
use crate::{device::UnicornContext, log_unsupported_read, log_unsupported_write};

pub const BASE: u64 = 0xb100d000;
pub const SIZE: usize = 0x1000;

const REG_BLTINTCR: u64 = 0xc;

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }
    match addr {
        REG_BLTINTCR => 1,
        _ => {
            log_unsupported_read!(addr, size);
            0
        }
    }
}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    if size != 4 {
        log_unsupported_write!(addr, size, value);
        return;
    }
    match addr {
        _ => {
            log_unsupported_write!(addr, size, value);
        }
    }
}
