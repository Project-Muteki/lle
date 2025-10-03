use core::fmt;
use std::{collections::HashMap, mem};

use log::{debug, error, info};
use unicorn_engine::Unicorn;

use crate::{extdev::sd::SD, peripherals::{aic, gpio, rtc, sic, sys, tmr, uart}};

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
    /// The default state. No reason to stop.
    #[default]
    Run,
    /// Quitting the emulator. Note that this has higher priority than ticks and will cancel a tick if there is one.
    Quit(QuitDetail),
    /// Ticking devices.
    Tick,
}

/// Extra emulator states.
///
/// Mostly contains the MMIO registers, but also some target-specific states (like stop reasons).
#[derive(Default)]
pub struct ExtraState {
    pub stop_reason: StopReason,
    pub steps: u64,

    pub store_only: HashMap<u64, u64>,
    pub clk: sys::ClockConfig,
    pub sic: sic::SICConfig,
    pub gpio: gpio::GPIOConfig,
    pub uart: uart::UARTConfig,
    pub rtc: rtc::RTCConfig,
    pub tmr: tmr::TimerConfig,
    pub aic: aic::AICConfig,
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

/// Defer a stop to right before the next instruction executes, stating the specified reason.
pub fn request_stop(uc: &mut UnicornContext, reason: StopReason) {
    let current_reason = &uc.get_data().stop_reason;
    if matches!(reason, StopReason::Run) {
        return;
    }
    if (
        matches!(current_reason, StopReason::Run) ||
        (!matches!(current_reason, StopReason::Quit(_)) && matches!(reason, StopReason::Quit(_)))
    ) {
        uc.get_data_mut().stop_reason = reason;
    }
}

/// Stops the emulator when a peripheral needs attention from the device emulator.
/// Called before the execution of every instruction.
pub fn check_stop_condition(uc: &mut UnicornContext, _addr: u64, _size: u32) {
    uc.get_data_mut().steps += 1;

    // TODO emulate actual clock behavior
    let steps = uc.get_data().steps;
    tmr::generate_stop_condition(uc, steps);

    // Collect either the 
    if !matches!(uc.get_data().stop_reason, StopReason::Run) {
        uc.emu_stop().unwrap_or_else(|err| {
            error!("Failed to stop emulator: {err:?}");
        });
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

        aic::tick(uc, self);
        sys::tick(uc, self);
        sic::tick(uc, self);
        gpio::tick(uc, self);
        uart::tick(uc, self);
        rtc::tick(uc, self);

        let prev_reason = mem::take(&mut uc.get_data_mut().stop_reason);
        match prev_reason {
            StopReason::Quit(reason) => {
                info!("Quit condition post-check: {reason}");
                false
            }
            _ => true
        }
    }
}
