use bit_field::{B1, B2, B4, B6, B8, bitfield};
use log::warn;

use crate::device::{Device, UnicornContext};
use crate::extdev::sd::Response;
use crate::peripherals::common::{log_unsupported_read, log_unsupported_write};

pub const NAME: &str = "SIC";
pub const NAME_DMAC: &str = "DMAC";
pub const NAME_FMI: &str = "FMI";
pub const BASE: u64 = 0xB1006000;
pub const SIZE: usize = 0x1000;

pub const BASE_DMAC: u64 = BASE;
pub const BASE_FMI: u64 = BASE + 0x800;

const REG_FB_0: u64 = BASE;
const REG_FB_0_END: u64 = BASE + 0x400;

const REG_DMACCSR: u64 = BASE + 0x400;
const REG_DMACSAR: u64 = BASE + 0x408;
const REG_DMACBCR: u64 = BASE + 0x40c;
const REG_DMACIER: u64 = BASE + 0x410;
const REG_DMACISR: u64 = BASE + 0x414;

const REG_FMICR: u64 = BASE_FMI;
const REG_FMIIER: u64 = BASE_FMI + 0x004;
const REG_FMIISR: u64 = BASE_FMI + 0x008;
const REG_SDCR: u64 = BASE_FMI + 0x020;
const REG_SDARG: u64 = BASE_FMI + 0x024;
const REG_SDIER: u64 = BASE_FMI + 0x028;
const REG_SDISR: u64 = BASE_FMI + 0x02c;
// Response[48:16]
const REG_SDRSP0: u64 = BASE_FMI + 0x030;
// Response[16:8] (excluding checksums)
const REG_SDRSP1: u64 = BASE_FMI + 0x034;
const REG_SDBLEN: u64 = BASE_FMI + 0x038;
const REG_SDTMOUT: u64 = BASE_FMI + 0x03c;

// NAND is not emulated

pub struct SICConfig {
    dma_dest_addr: Option<u64>,
    sd_arg: u32,
    sd_control: SDCR,
    sd_response: (u32, u32),
    fifo: [u8; 0x400],
}

impl Default for SICConfig {
    fn default() -> Self {
        Self {
            dma_dest_addr: None,
            sd_arg: 0u32,
            sd_control: Default::default(),
            sd_response: Default::default(),
            fifo: [0u8; 1024],
        }
    }
}

#[bitfield]
#[derive(Default)]
struct SDCR {
    co_en: B1,
    ri_en: B1,
    di_en: B1,
    do_en: B1,
    r2_en: B1,
    clk74_oe: B1,
    clk8_oe: B1,
    clk_keep: B1,
    cmd_code: B6,
    swrst: B1,
    dbw: B1,
    blkcnt: B8,
    sdnwr: B4,
    clk_keep2: B1,
    sdport: B2,
    clk_keep1: B1,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if addr >= REG_FB_0 && addr < REG_FB_0_END {
        // TODO
        let fifo = &uc.get_data().sic.fifo;
        let fifo_addr: usize = (((addr - REG_FB_0) & 0x3ff) as u16).into();
        return match size {
            1 => fifo[fifo_addr].into(),
            2 => {
                if addr & 1 != 0 {
                    warn!("{NAME_DMAC}: Unaligned read16 at address {addr}.");
                    return 0;
                }
                (fifo[fifo_addr] | fifo[fifo_addr + 1] << 8).into()
            }
            4 => {
                if addr & 3 != 0 {
                    warn!("{NAME_DMAC}: Unaligned read32 at address {addr}.");
                    return 0;
                }
                (fifo[fifo_addr] |
                    (fifo[fifo_addr + 1] << 8) |
                    (fifo[fifo_addr + 2] << 16) |
                    (fifo[fifo_addr + 3] << 24)).into()
            }
            _ => {
                log_unsupported_read(NAME, addr, size);
                0
            }
        };
    }
    if size != 4 {
        log_unsupported_read(NAME, addr, size);
        return 0;
    }
    let sic = &uc.get_data().sic;
    match addr {
        REG_DMACSAR => {
            match sic.dma_dest_addr {
                Some(v) => v,
                None => 0u64,
            }
        }
        _ => {
            log_unsupported_read(NAME, addr, size);
            0
        }
    }

}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    let data = uc.get_data_mut();

    if addr >= REG_FB_0 && addr < REG_FB_0_END {
        // TODO
        let fifo = &mut data.sic.fifo;
        let fifo_addr: usize = (((addr - REG_FB_0) & 0x3ff) as u16).into();
        match size {
            1 => fifo[fifo_addr] = value as u8,
            2 => {
                if addr & 1 != 0 {
                    warn!("{NAME_DMAC}: Unaligned write16 at address {addr}.");
                    return;
                }
                fifo[fifo_addr] = (value & 0xff) as u8;
                fifo[fifo_addr + 1] = (value >> 8) as u8;
            }
            4 => {
                if addr & 3 != 0 {
                    warn!("{NAME_DMAC}: Unaligned write32 at address {addr}.");
                    return;
                }
                fifo[fifo_addr] = (value & 0xff) as u8;
                fifo[fifo_addr + 1] = (value >> 8) as u8;
                fifo[fifo_addr + 2] = (value >> 16) as u8;
                fifo[fifo_addr + 3] = (value >> 24) as u8;
            }
            _ => log_unsupported_write(NAME, addr, size, value),
        };
    }
    if size != 4 {
        log_unsupported_write(NAME, addr, size, value);
        return;
    }
    let sic = &mut data.sic;
    match addr {
        REG_SDARG => sic.sd_arg = value as u32,
        REG_SDCR => {
            let sd_control = &mut sic.sd_control;
            sd_control.set(0, 32, value);
        }
        _ => log_unsupported_write(NAME, addr, size, value),
    }
}

pub fn tick(uc: &mut UnicornContext, device: &mut Device) {
    // Do not tick if clock is disabled
    if uc.get_data().clk.ahbclk.get_sic() == 0 {
        return;
    }

    let sd_control = &uc.get_data().sic.sd_control;
    let command_enable = sd_control.get_co_en() == 1;
    let sd_port = sd_control.get_sdport();
    let has_data_in = sd_control.get_di_en() == 1;
    let has_data_out = sd_control.get_do_en() == 1;

    let cmd = sd_control.get_cmd_code();
    let arg = uc.get_data().sic.sd_arg;

    if command_enable {
        let sd_device_op = match sd_port {
            0 => Some(&mut device.internal_sd),
            2 => Some(&mut device.external_sd),
            _ => None
        };
        match sd_device_op {
            Some(sd_device) => {
                let sic_mut = &mut uc.get_data_mut().sic;
                match sd_device.make_request(cmd, arg) {
                    Response::R1(response_type1) => {
                        sic_mut.sd_response = response_type1.into();
                        sic_mut.sd_control.set_ri_en(0);
                        sic_mut.sd_control.set_co_en(0);
                    },
                    Response::R2(response_type2) => {
                        sic_mut.fifo[..response_type2.cid_csd.len()].copy_from_slice(&response_type2.cid_csd);
                        sic_mut.sd_control.set_r2_en(0);
                        sic_mut.sd_control.set_co_en(0);
                    },
                    Response::R3(response_type3) => {
                        sic_mut.sd_response = response_type3.into();
                        sic_mut.sd_control.set_ri_en(0);
                        sic_mut.sd_control.set_co_en(0);
                    },
                    Response::R6(response_type6) => {
                        sic_mut.sd_response = response_type6.into();
                        sic_mut.sd_control.set_ri_en(0);
                        sic_mut.sd_control.set_co_en(0);
                    },
                    Response::R7(response_type7) => {
                        sic_mut.sd_response = response_type7.into();
                        sic_mut.sd_control.set_ri_en(0);
                        sic_mut.sd_control.set_co_en(0);
                    },
                    Response::RNone => {},
                }
            }
            None => {
                warn!("{NAME}: Cannot send command through unmapped SD port {sd_port}");
            }
        }
    }

    if has_data_in {
        todo!();
    }

    if has_data_out {
        todo!();
    }
}
