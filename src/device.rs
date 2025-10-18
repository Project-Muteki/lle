use core::fmt;
use std::{collections::HashMap, iter::zip, mem};

use bitflags::bitflags;
use log::{error, info, trace};
use pixels::Pixels;
use unicorn_engine::Unicorn;

use crate::{exception::{ExceptionType, call_exception_handler}, extdev::{input::Input, sd::SD}, peripherals::{adc, aic, blt, gpio, rtc, sic, sys, tmr, uart, vpost}};

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

bitflags! {
    #[derive(Debug, Default, Copy, Clone)]
    pub struct StopReason: u8 {
        /// Ticking devices.
        const Tick = 1 << 0;
        const FrameStep = 1 << 1;
        const SVC = 1 << 2;
    }
}

/// Extra emulator states.
///
/// Mostly contains the MMIO registers, but also some target-specific states (like stop reasons).
#[derive(Default)]
pub struct ExtraState {
    pub raw_sdram: Vec<u8>,
    pub stop_reason: StopReason,
    pub quit_detail: Option<QuitDetail>,
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
    pub input: Input,
}

pub type UnicornContext<'a> = Unicorn<'a, Box<ExtraState>>;

#[inline]
/// Defer a stop to right before the next instruction executes, stating the specified reason.
pub fn request_stop(uc: &mut UnicornContext, reason: StopReason) {
    uc.get_data_mut().stop_reason |= reason;
}

#[inline]
pub fn request_quit(uc: &mut UnicornContext, detail: QuitDetail) {
    uc.get_data_mut().quit_detail = Some(detail);
}

/// Stops the emulator when a peripheral needs attention from the device emulator.
/// Called before the execution of every instruction.
pub fn check_stop_condition(uc: &mut UnicornContext, _addr: u64, _size: u32) {
    uc.get_data_mut().steps += 1;

    // TODO emulate actual clock behavior
    let steps = uc.get_data().steps;
    vpost::generate_stop_condition(uc, steps);
    tmr::generate_stop_condition(uc, steps);

    if !uc.get_data().stop_reason.is_empty() {
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
        let quit_detail = mem::take(&mut uc.get_data_mut().quit_detail);
        if let Some(reason) = quit_detail {
            info!("Quit condition pre-check: {reason}");
            return false;
        }

        let reason = mem::take(&mut uc.get_data_mut().stop_reason);

        if reason.contains(StopReason::FrameStep) {
            adc::frame_step(uc);
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
                    request_quit(uc, QuitDetail::HLECallbackFailure);
                },
            }
        }

        if reason.contains(StopReason::SVC) {
            call_exception_handler(uc, ExceptionType::SupervisorCall).unwrap_or_else(|err| {
                error!("Failed to invoke exception handler: {err:?}.");
            });
        }

        if reason.contains(StopReason::Tick) {
            aic::tick(uc);
            sys::tick(uc);
            rtc::tick(uc);
            sic::tick(uc, self);
            blt::tick(uc);
            adc::tick(uc, self);
        }

        let quit_detail = mem::take(&mut uc.get_data_mut().quit_detail);
        if let Some(reason) = quit_detail {
            info!("Quit condition post-check: {reason}");
            false
        } else {
            true
        }
    }
}
