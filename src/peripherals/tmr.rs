use bit_field::{B2, B8, bitfield};
use log::{trace, warn};
use crate::{device::UnicornContext, log_unsupported_read, log_unsupported_write, peripherals::aic::{InterruptNumber, post_interrupt}};

pub const BASE: u64 = 0xb8002000;
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
    alive: bool,
    auto_reset_enabled: bool,
    reset_flag: bool,
    irq_enabled: bool,
    interval: B2,
    irq_handler_installed: bool,
    enabled: bool,
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
    tdr_en: bool,
    reserved_17: B8,
    is_active: bool,
    reset: bool,
    mode: TimerMode,
    irq_enable: bool,
    enable: bool,
    dbgack_en: bool,
}


#[derive(Default)]
pub struct TimerChannel {
    pub count: u32,
    pub compare: u32,
    pub control: TimerControl,
    /// Toggle out
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
        if !self.control.get_enable() {
            return false;
        }

        if self.count == self.compare {
            let mut rv = true;
            match self.control.get_mode() {
                TimerMode::OneShot => self.control.set_enable(false),
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

    #[inline]
    pub fn reset(&mut self) {
        self.control.set_reset(false);
        self.control.set_enable(false);
        self.reset_counter();
    }

    #[inline]
    pub fn reset_counter(&mut self) {
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
            if uc.get_data().tmr.channels[0].control.get_reset() {
                uc.get_data_mut().tmr.channels[0].reset();
            }
            if uc.get_data().tmr.channels[0].control.get_enable() {
                let pc = uc.pc_read().unwrap();
                trace!("TMR0 enable @ 0x{pc:08x}");
            }
            if uc.get_data().tmr.channels[0].control.get_irq_enable() { 
                let pc = uc.pc_read().unwrap();
                trace!("TMR0 IRQ enable @ 0x{pc:08x}");
            }
            trace!("REG_TCSR0 {:?}", uc.get_data().tmr.channels[0].control);
        }
        REG_TCSR1 => {
            uc.get_data_mut().tmr.channels[1].control.set(0, 32, value);
            if uc.get_data().tmr.channels[1].control.get_reset() {
                uc.get_data_mut().tmr.channels[1].reset();
            }
            trace!("REG_TCSR1 {:?}", uc.get_data().tmr.channels[1].control);
        }
        REG_TICR0 => {
            uc.get_data_mut().tmr.channels[0].compare = u32::try_from(value & 0xffffffff).unwrap();
            uc.get_data_mut().tmr.channels[0].reset_counter();
            trace!("REG_TICR0 {:?}", uc.get_data().tmr.channels[0].compare);
        },
        REG_TICR1 => {
            uc.get_data_mut().tmr.channels[1].compare = u32::try_from(value & 0xffffffff).unwrap();
            uc.get_data_mut().tmr.channels[1].reset_counter();
            trace!("REG_TICR1 {:?}", uc.get_data().tmr.channels[1].compare);
        }
        REG_TISR => uc.get_data_mut().tmr.status &= !u8::try_from(value & 0xff).unwrap(),
        REG_WTCR => uc.get_data_mut().tmr.watchdog.set(0, 8, value),
        _ => log_unsupported_write!(addr, size, value),
    }
    
}

pub fn generate_stop_condition(uc: &mut UnicornContext, steps: u64) {
    let div_apb = uc.get_data().clk.tick_config.apb;
    if steps % div_apb != 0 {
        return;
    }

    for timer in &mut uc.get_data_mut().tmr.channels {
        if !timer.control.get_enable() {
            continue;
        }
        let rate = div_apb * (u64::from(timer.control.get_prescale()) + 1);
        if steps % rate == 0 {
            timer.count += 1;
        }
    }

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
