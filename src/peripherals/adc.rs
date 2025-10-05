use bit_field::{B1, B7, B8, bitfield};
use log::warn;
use crate::{device::{Device, UnicornContext}, log_unsupported_read, log_unsupported_write};

pub const BASE: u64 = 0xB800E000;
pub const SIZE: usize = 0x1000;

const ADC_CON: u64 = 0x0;
const ADC_XDATA: u64 = 0xc;
const ADC_YDATA: u64 = 0x10;

#[bitfield]
#[derive(Debug, PartialEq)]
pub enum ADCMux {
    MicPos,
    MicNeg,
    AIn2,
    AIn3,
    AIn4,
    TouchX,
    TouchY,
    Reserved7,
}

#[bitfield]
#[derive(Debug, PartialEq)]
pub enum TouchMode {
    Manual,
    SemiAuto,
    Auto,
    WaitForTrigger,
}

#[bitfield]
pub struct ADCControl {
    done: bool,
    reserved_1: B7,
    /// true - audio sample is raw, false - audio sample is filtered.
    audio_raw: bool,
    mux: ADCMux,
    /// Start the next sampling automatically after a sample is read.
    streaming: bool,
    /// Write true to start sampling.
    start_sample: bool,
    touch_mode: TouchMode,
    reset: bool,
    enable: bool,
    irq_status: bool,
    reserved_19: B1,
    wait_for_trigger_status: bool,
    irq_enable: bool,
    reserved_22: B1,
    wait_for_trigger_enable: bool,
    reserved_24: B8,
}

impl Default for ADCControl {
    fn default() -> Self {
        let mut result = Self::new();
        result.set_done(true);
        result
    }
}

#[derive(Default)]
pub struct ADCConfig {
    pub control: ADCControl,
    pub xdata: u16,
    pub ydata: u16,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    let adc = &uc.get_data().adc;

    match addr {
        ADC_CON => adc.control.get(0, 32),
        ADC_XDATA => adc.xdata.into(),
        ADC_YDATA => adc.ydata.into(),
        _ => {
            log_unsupported_read!(addr, size);
            0
        }
    }
}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    if size != 4 {
        log_unsupported_write!(addr, size, value);
    }

    let adc = &mut uc.get_data_mut().adc;

    match addr {
        ADC_CON => adc.control.set(0, 32, value),
        _ => {
            log_unsupported_write!(addr, size, value);
        }
    }
}

pub fn tick(_uc: &mut UnicornContext, _device: &mut Device) {

}
