use core::fmt;
use std::collections::HashMap;

use log::info;
use unicorn_engine::Unicorn;

use crate::{extdev::sd::SD, peripherals::{gpio, rtc, sic, sys, tmr, uart}};

#[derive(Default, Debug, PartialEq)]
pub enum QuitDetail {
    #[default]
    UserSpecified,
    CPUException,
    CPUHalt,
}

impl fmt::Display for QuitDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserSpecified => { write!(f, "Stopping emulator...") }
            Self::CPUException => { write!(f, "CPU exception occurred and user asked us to quit on this type of exception.") }
            Self::CPUHalt => { write!(f, "CPU halted.") }
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub enum StopReason {
    #[default]
    Run,
    Quit(QuitDetail),
    TickDevice,
}

/// Extra emulator states.
///
/// Mostly contains the MMIO registers, but also some target-specific states (like stop reasons).
#[derive(Default)]
pub struct ExtraState {
    pub stop_reason: StopReason,
    pub store_only: HashMap<u64, u64>,
    pub clk: sys::ClockConfig,
    pub sic: sic::SICConfig,
    pub gpio: gpio::GPIOConfig,
    pub uart: uart::UARTConfig,
    pub rtc: rtc::RTCConfig,
    pub tmr: tmr::TimerConfig,
}

/// Peripheral device emulation context.
///
/// Contains the states required to emulate devices, and actual device logic (excluding MMIO, which is considered part
/// of the emulator state).
#[derive(Default)]
pub struct Device {
    pub internal_sd: SD,
    pub external_sd: SD,
}

pub type UnicornContext<'a> = Unicorn<'a, ExtraState>;

/// Stops the emulator when a peripheral needs attention from the device emulator. Called after execution of every
/// instruction.
pub fn check_stop_condition(uc: &mut UnicornContext, _addr: u64, _size: u32) {
    let data = uc.get_data_mut();
    data.clk.ticks += 1;
    // TODO emulate actual clock behavior
    if data.clk.ticks % 2 == 0 {
        uc.emu_stop().unwrap();
    }
}

impl Device {
    /// Process MMIO register updates and device state changes.
    ///
    /// This will modify both the device states and the emulator states associated with it.
    pub fn tick(&mut self, uc: &mut UnicornContext) -> bool {
        match &uc.get_data().stop_reason {
            StopReason::Quit(reason) => {
                info!("Quit condition pre-check: {reason}");
                return false;
            }
            _ => {}
        }

        sys::tick(uc, self);
        sic::tick(uc, self);
        gpio::tick(uc, self);
        uart::tick(uc, self);
        rtc::tick(uc, self);

        match &uc.get_data().stop_reason {
            StopReason::Quit(reason) => {
                info!("Quit condition post-check: {reason}");
                false
            }
            _ => true
        }
    }
}
