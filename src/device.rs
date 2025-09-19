use std::collections::HashMap;

use unicorn_engine::Unicorn;

use crate::{extdev::sd::SD, peripherals::{sic::SICConfig, sys::ClockConfig}};

#[derive(Default)]
pub struct PeripheralState {
    pub store_only: HashMap<u64, u64>,
    pub clk: ClockConfig,
    pub sic: SICConfig,
}

#[derive(Default)]
pub struct Device {
    pub internal_sd: SD,
    pub external_sd: SD,
}

pub type UnicornContext<'a> = Unicorn<'a, PeripheralState>;

impl Device {
    pub fn device_tick(&mut self, uc: &mut UnicornContext) {
        
    }
}
