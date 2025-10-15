use core::fmt;
use std::{collections::HashMap, iter::zip, mem};

use log::{error, info, trace};
use pixels::Pixels;
use unicorn_engine::Unicorn;

use crate::{exception::{ExceptionType, call_exception_handler}, extdev::sd::SD, peripherals::{adc, aic, blt, gpio, rtc, sic, sys, tmr, uart, vpost}};

#[derive(Default, Debug, PartialEq)]
pub enum QuitDetail {
    #[default]
    UserSpecified,
    CPUException,
    CPUHalt,
    HLECallbackFailure,
}

impl fmt::Display for QuitDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserSpecified => { write!(f, "Stopping emulator...") }
            Self::CPUException => { write!(f, "CPU exception occurred and user asked us to quit on this type of exception.") }
            Self::CPUHalt => { write!(f, "CPU halted.") }
            Self::HLECallbackFailure => { write!(f, "HLE callback failed to execute.") }
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
    FrameStep,
    SVC,
}

/// Extra emulator states.
///
/// Mostly contains the MMIO registers, but also some target-specific states (like stop reasons).
#[derive(Default)]
pub struct ExtraState {
    pub raw_sdram: Vec<u8>,
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
    pub adc: adc::ADCConfig,
    pub vpost: vpost::LCDConfig,
    pub blt: blt::BLTConfig,
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

pub type UnicornContext<'a> = Unicorn<'a, Box<ExtraState>>;

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
    vpost::generate_stop_condition(uc, steps);
    tmr::generate_stop_condition(uc, steps);

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
    pub fn tick(&mut self, uc: &mut UnicornContext, render: &mut Pixels) -> bool {
        match &uc.get_data().stop_reason {
            StopReason::Run => {}
            StopReason::Quit(reason) => {
                info!("Quit condition pre-check: {reason}");
                return false;
            },
            StopReason::FrameStep => {
                if uc.get_data().vpost.control.get_run() {
                    trace!("Frame copy from 0x{:08x}", uc.get_data().vpost.fb);
                    let a = uc.mem_read_as_vec(uc.get_data().vpost.fb.into(), 320 * 240 * 2).unwrap();
                    for (spx, dpx) in zip(a.chunks_exact(2), render.frame_mut().chunks_exact_mut(4)) {
                        dpx[0] = spx[1] & 0b11111000;
                        dpx[1] = ((spx[1] & 0b111) << 5) | ((spx[0] & 0b11100000) >> 3);
                        dpx[2] = spx[0] << 3;
                        dpx[3] = 0xff;
                    }
                }
                match render.render() {
                    Ok(_) => {}
                    Err(err) => {
                        error!("Failed to render image: {err:?}");
                        uc.get_data_mut().stop_reason = StopReason::Quit(QuitDetail::HLECallbackFailure);
                    },
                }
            }
            StopReason::SVC => {
                call_exception_handler(uc, ExceptionType::SupervisorCall).unwrap_or_else(|err| {
                    error!("Failed to invoke exception handler: {err:?}.");
                });
            }
            StopReason::Tick => {
                aic::tick(uc);
                sys::tick(uc);
                rtc::tick(uc);
                sic::tick(uc, self);
                blt::tick(uc);
            }
        }


        let prev_reason = mem::take(&mut uc.get_data_mut().stop_reason);
        if let StopReason::Quit(reason) = prev_reason {
            info!("Quit condition post-check: {reason}");
            false
        } else {
            true
        }
    }
}
