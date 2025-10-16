use bit_field::{B1, B6, B7, B8, bitfield};
use log::{trace, warn};
use crate::{device::{Device, UnicornContext}, log_unsupported_read, log_unsupported_write, peripherals::aic::{InterruptNumber, post_interrupt}};

pub const BASE: u64 = 0xb800e000;
pub const SIZE: usize = 0x1000;

const ADC_CON: u64 = 0x0;
const ADC_TSC: u64 = 0x4;
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
#[derive(Debug, PartialEq)]
pub enum TouchscreenType {
    FourWire,
    FiveWire,
    EightWire,
    Reserved3,
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

#[bitfield]
#[derive(Default)]
pub struct ADCTouchControl {
    pressing: bool,
    touchscreen_type: TouchscreenType,
    pullup: bool,
    manual_short_ym: bool,
    manual_short_yp: bool,
    manual_short_xm: bool,
    manual_short_xp: bool,
    semiauto_xy_detection: bool,
    auto_filter: bool,
    reserved_10: B6,
}

#[derive(Default)]
pub struct ADCConfig {
    pub control: ADCControl,
    pub touch_control: ADCTouchControl,
    pub xdata: u16,
    pub ydata: u16,

    pub touch_x: u16,
    pub touch_y: u16,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    let adc = &uc.get_data().adc;

    match addr {
        ADC_CON => adc.control.get(0, 32),
        ADC_TSC => adc.touch_control.get(0, 16),
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
        return;
    }

    let adc = &mut uc.get_data_mut().adc;

    match addr {
        ADC_CON => {
            adc.control.set(0, 32, value);
            if value & (1 << 18) != 0 {
                adc.control.set_irq_status(false);
            }
            if value & (1 << 20) != 0 {
                adc.control.set_wait_for_trigger_status(false);
            }
            if adc.control.get_start_sample() {
                trace!("ADC sample with {:?}", adc.control);
                adc.control.set_start_sample(false);
                adc.control.set_irq_status(true);
                match adc.control.get_mux() {
                    ADCMux::MicPos => {
                        if adc.control.get_touch_mode() == TouchMode::Auto {
                            adc.xdata = adc.touch_x;
                            adc.ydata = adc.touch_y;
                        }
                    },
                    ADCMux::AIn2 => {
                        adc.xdata = 1023;
                        adc.ydata = 0;
                    }
                    _ => {
                        adc.xdata = 0;
                        adc.ydata = 0;
                    }
                }
            }
        }
        ADC_TSC => adc.touch_control.set(1, 15, value >> 1),
        _ => {
            log_unsupported_write!(addr, size, value);
        }
    }
}

pub fn frame_step(uc: &mut UnicornContext, device: &mut Device) {
    if !(uc.get_data().clk.apbclk.get_adc() && uc.get_data().adc.control.get_enable()) {
        return;
    }

    //trace!("frame step {:?}", uc.get_data().adc.control);

    if uc.get_data().adc.control.get_touch_mode() == TouchMode::WaitForTrigger &&
        let Some(update) = device.input.check_touch()
    {
        trace!("Touch triggered");
        let adc = &mut uc.get_data_mut().adc;
        if let Some(pos) = update {
            adc.touch_x = (24.0 + ((pos.0 as f64 / 319.0) * 967.0)).round() as u16;
            adc.touch_y = (24.0 + (((239 - pos.1) as f64 / 239.0) * 967.0)).round() as u16;
            adc.control.set_wait_for_trigger_status(true);
            adc.touch_control.set_pressing(true);
            trace!("New x={} y={}", adc.touch_x, adc.touch_y);
        } else {
            adc.control.set_wait_for_trigger_status(true);
            adc.touch_control.set_pressing(false);
            trace!("Release");
        }

        if uc.get_data().adc.control.get_wait_for_trigger_enable() {
            post_interrupt(uc, InterruptNumber::ADC, true, false);
        }
    }
}
