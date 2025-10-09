use bit_field::{B3, B7, B12, bitfield};
use log::{error, warn};
use crate::{device::{QuitDetail, StopReason, UnicornContext, request_stop}, log_unsupported_read, log_unsupported_write};

pub const BASE: u64 = 0xb1002000;
pub const SIZE: usize = 0x1000;

const LCDC_PRM: u64 = 0x4;

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

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }
    match addr {
        LCDC_PRM => 0x4384_8900,
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
        LCDC_PRM => {
            // TODO
            let pc = uc.pc_read().unwrap();
            error!("LCDC_PRM = 0x{value:08x} pc = 0x{pc:08x}");
            request_stop(uc, StopReason::Quit(QuitDetail::UserSpecified));
        }
        _ => {
            log_unsupported_write!(addr, size, value);
        }
    }
}
