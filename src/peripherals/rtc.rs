use log::warn;

use crate::{device::{Device, UnicornContext}, log_unsupported_read, log_unsupported_write};

pub const BASE: u64 = 0xB8003000;
pub const SIZE: usize = 0x1000;

const REG_INIR: u64 = 0x0;
const REG_AER: u64 = 0x4;

const MAGIC_INIT: u32 = 0xa5eb1357;
const MAGIC_WRITE: u16 = 0xa965;

#[derive(Default)]
pub struct RTCConfig {
    enabled: bool,
    write_enabled: bool,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    match addr {
        REG_AER => if uc.get_data().rtc.write_enabled { 0x10000 } else { 0x0 }
        _ => {
            log_unsupported_read!(addr, size);
            0
        }
    }
}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    if addr == REG_AER {
        uc.get_data_mut().rtc.write_enabled = true;
        return;
    } else if !uc.get_data().rtc.write_enabled {
        warn!("Register 0x{addr:x} is write protected.");
    }
    log_unsupported_write!(addr, size, value);
    uc.get_data_mut().rtc.write_enabled = false;
}

pub fn tick(_uc: &mut UnicornContext, _device: &mut Device) {

}
