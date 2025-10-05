use bit_field::{B1, B2, B8, bitfield};
use log::warn;
use crate::{device::{Device, UnicornContext}, log_unsupported_read, log_unsupported_write, peripherals::aic::{InterruptNumber, post_interrupt}};

pub const BASE: u64 = 0xB8002000;
pub const SIZE: usize = 0x1000;

const REG_TCSR0: u64 = 0x0;
const REG_TCSR1: u64 = 0x4;
const REG_TICR0: u64 = 0x8;
const REG_TICR1: u64 = 0xc;
const REG_TDR0: u64 = 0x10;
const REG_TDR1: u64 = 0x14;
const REG_TISR: u64 = 0x18;
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

#[bitfield]
#[derive(Default, Debug, PartialEq)]
pub enum TimerMode {
    #[default]
    OneShot,
    Periodic,
    Toggle,
    Uninterrupted,
}

#[bitfield]
#[derive(Default)]
pub struct TimerControl {
    prescale: B8,
    reserved_8: B8,
    tdr_en: B1,
    reserved_17: B8,
    is_active: B1,
    reset: B1,
    mode: TimerMode,
    irq_enable: B1,
    enable: B1,
    dbgack_en: B1,
}


#[derive(Default)]
pub struct TimerChannel {
    pub count: u32,
    pub compare: u32,
    pub control: TimerControl,
    pub level: bool,
}

#[derive(Default)]
pub struct TimerConfig {
    pub status: u8,
    pub channels: [TimerChannel; 2],
    pub watchdog: WatchdogControl,
}

impl TimerChannel {
    pub fn check_irq_condition(&mut self) -> bool {
        if self.control.get_enable() == 0 {
            return false;
        }

        if self.count == self.compare {
            let mut rv = true;
            match self.control.get_mode() {
                TimerMode::OneShot => self.control.set_enable(0),
                TimerMode::Periodic => self.count = 0,
                TimerMode::Toggle => {
                    self.count = 0;
                    if self.level {
                        rv = false;
                        self.level = !self.level;
                    }
                },
                TimerMode::Uninterrupted => {},
            }
            rv
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.control.set_reset(0);
        self.control.set_enable(0);
        self.count = 0;
        self.level = false;
    }
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    match addr {
        REG_TCSR0 => uc.get_data().tmr.channels[0].control.get(0, 32),
        REG_TCSR1 => uc.get_data().tmr.channels[1].control.get(0, 32),
        REG_TICR0 => uc.get_data().tmr.channels[0].compare.into(),
        REG_TICR1 => uc.get_data().tmr.channels[1].compare.into(),
        REG_TDR0 => uc.get_data().tmr.channels[0].count.into(),
        REG_TDR1 => uc.get_data().tmr.channels[1].count.into(),
        REG_TISR => uc.get_data().tmr.status.into(),
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
        REG_TCSR0 => {
            uc.get_data_mut().tmr.channels[0].control.set(0, 32, value);
            if uc.get_data().tmr.channels[0].control.get_reset() == 1 {
                uc.get_data_mut().tmr.channels[0].reset();
            }
        }
        REG_TCSR1 => {
            uc.get_data_mut().tmr.channels[1].control.set(0, 32, value);
            if uc.get_data().tmr.channels[1].control.get_reset() == 1 {
                uc.get_data_mut().tmr.channels[1].reset();
            }
        }
        REG_TICR0 => uc.get_data_mut().tmr.channels[0].compare = u32::try_from(value & 0xffffffff).unwrap(),
        REG_TICR1 => uc.get_data_mut().tmr.channels[1].compare = u32::try_from(value & 0xffffffff).unwrap(),
        REG_TISR => uc.get_data_mut().tmr.status &= !u8::try_from(value & 0xff).unwrap(),
        REG_WTCR => uc.get_data_mut().tmr.watchdog.set(0, 8, value),
        _ => log_unsupported_write!(addr, size, value),
    }
    
}

pub fn tick(_uc: &mut UnicornContext, device: &mut Device) {

}

pub fn generate_stop_condition(uc: &mut UnicornContext, steps: u64) {
    let div_apb = uc.get_data().clk.tick_config.apb;
    if steps % div_apb != 0 {
        return;
    }

    for timer in &mut uc.get_data_mut().tmr.channels {
        if timer.control.get_enable() == 0 {
            continue;
        }
        let rate = div_apb * (u64::from(timer.control.get_prescale()) + 1);
        if steps % rate == 0 {
            timer.count += 1;
        }
    }

    // TODO: This is not exactly correct: the correct way would be passing the level variable and the change to the post_interrupt callback.
    if uc.get_data_mut().tmr.channels[0].check_irq_condition() {
        uc.get_data_mut().tmr.status |= 0x1;
        post_interrupt(uc, InterruptNumber::TMR0, true, false);
    }

    if uc.get_data_mut().tmr.channels[1].check_irq_condition() {
        uc.get_data_mut().tmr.status |= 0x2;
        post_interrupt(uc, InterruptNumber::TMR1, true, false);
    }
    // TODO
}
