use std::collections::HashMap;

use unicorn_engine::Unicorn;

use crate::{extdev::sd::SD, peripherals::{gpio, sic, sys}};

#[derive(Default)]
pub struct MMIOState {
    pub store_only: HashMap<u64, u64>,
    pub clk: sys::ClockConfig,
    pub sic: sic::SICConfig,
    pub gpio: gpio::GPIOConfig,
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

pub type UnicornContext<'a> = Unicorn<'a, MMIOState>;

/// Stop the emulator on peripheral clock edge. Called after execution of every instruction.
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
    pub fn tick(&mut self, uc: &mut UnicornContext) {
        sic::tick(uc, self);
    }
}
