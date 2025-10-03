use bit_field::{B1, B2, bitfield};
use log::warn;
use crate::{device::{Device, UnicornContext}, log_unsupported_read, log_unsupported_write, peripherals::sys::F_BASE};

pub const BASE: u64 = 0xB8002000;
pub const SIZE: usize = 0x1000;

const REG_WTCR: u64 = 0x1c;

#[bitfield]
#[derive(Default)]
pub struct WatchdogControl {
    alive: B1,
    auto_reset_enabled: B1,
    reset_flag: B1,
    irq_enabled: B1,
    interval: B2,
    irq_handler_installed: B1,
    enabled: B1,
}

#[derive(Default)]
pub struct ChannelControl {
    pub count: u32,
    pub compare: u32,
}

#[derive(Default)]
pub struct TimerConfig {
    pub channels: [ChannelControl; 2],
    pub watchdog: WatchdogControl,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    match addr {
        REG_WTCR => uc.get_data().tmr.watchdog.get(0, 8),
        _ => {
            log_unsupported_read!(addr, size);
            0
        },
    }
}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    if size != 4 {
        log_unsupported_write!(addr, size, value);
        return;
    }

    match addr {
        REG_WTCR => uc.get_data_mut().tmr.watchdog.set(0, 8, value),
        _ => log_unsupported_write!(addr, size, value),
    }
    
}

pub fn tick(_uc: &mut UnicornContext, _device: &mut Device) {

}

pub fn generate_stop_condition(uc: &mut UnicornContext, steps: u64) {
    if steps % uc.get_data().clk.tick_config.apb != 0 {
        return;
    }

    // TODO
}
