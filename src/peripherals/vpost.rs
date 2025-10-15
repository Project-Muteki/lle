use bit_field::{B2, B3, B7, B8, B12, bitfield};
use log::{trace, warn};
use crate::{device::{StopReason, UnicornContext, request_stop}, log_unsupported_read, log_unsupported_write, peripherals::common::{mmio_get_store_only, mmio_set_store_only}};

pub const BASE: u64 = 0xb1002000;
pub const SIZE: usize = 0x1000;

const LCDC_CTL: u64 = 0x0;
const LCDC_PRM: u64 = 0x4;
const LCDC_INT: u64 = 0x8;
const TCON1: u64 = 0x10;
const TCON2: u64 = 0x14;
const TCON3: u64 = 0x18;
const TCON4: u64 = 0x1c;

const FSADDR: u64 = 0x50;

#[bitfield]
#[derive(Debug, PartialEq)]
pub enum FrameBufferFormat {
    RGB555,
    RGB565,
    XRGB,
    RGBX,
    Cb0Y0Cr0Y1,
    Y0Cb0Y1Cr0,
    Cr0Y0Cb0Y1,
    Y0Cr0Y1Cb0,
}

#[bitfield]
#[derive(Debug, PartialEq)]
pub enum ParallelRGBBusType {
    RGB565,
    RGB666,
    RGB888,
    Reserved3,
}

#[bitfield]
#[derive(Default)]
pub struct LCDControl {
    run: bool,
    fb_format: FrameBufferFormat,
    reserved_4: B12,
    yuv_le: bool,
    reserved_17: B3,
    parallel_rgb_bus_type: ParallelRGBBusType,
    reserved_22: B7,
    polyphase_filter: bool,
    haw_656: bool,
    use_fsc: bool,
}

#[bitfield]
#[derive(Default)]
pub struct LCDIRQStatus {
    hsync: bool,
    vsync: bool,
    tvfield_0: bool,
    tvfield_1: bool,
    ext_controller_frame_done: bool,
    reserved_5: B2,
    underrun: bool,
    reserved_8: B8,
    hsync_enable: bool,
    vsync_enable: bool,
    reserved_18: B2,
    ext_controller_frame_done_enable: bool,
    reserved_21: B2,
    clear_underrun: bool,
    reserved_24: B8,
}

#[derive(Default)]
pub struct LCDConfig {
    pub control: LCDControl,
    pub irq: LCDIRQStatus,
    pub fb: u32,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }
    match addr {
        LCDC_CTL => uc.get_data().vpost.control.get(0, 32),
        LCDC_INT => uc.get_data().vpost.irq.get(0, 32),
        LCDC_PRM | TCON1 | TCON2 | TCON3 | TCON4 => mmio_get_store_only(uc, BASE + addr),
        FSADDR => uc.get_data().vpost.fb.into(),
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
    match addr {
        LCDC_CTL => {
            trace!("LCDCCtl = 0x{:08x}", value);
            uc.get_data_mut().vpost.control.set(0, 32, value);
        },
        LCDC_INT => {
            trace!("LCDCInt = 0x{:08x}", value);
            uc.get_data_mut().vpost.irq.set(0, 32, value);
        },
        LCDC_PRM | TCON1 | TCON2 | TCON3 | TCON4 => mmio_set_store_only(uc, BASE + addr, value),
        FSADDR => {
            uc.get_data_mut().vpost.fb = value as u32;
        }
        _ => {
            log_unsupported_write!(addr, size, value);
        }
    }
}

pub fn generate_stop_condition(uc: &mut UnicornContext, steps: u64) {
    let div_vsync = uc.get_data().clk.tick_config.vsync;
    if steps % div_vsync == 0 {
        request_stop(uc, StopReason::FrameStep);
    }
}
