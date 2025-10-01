use bit_field::{B1, B2, B3, B4, B5, B6, B7, B8, B9, bitfield};
use log::{debug, error, trace, warn};

use crate::device::{Device, UnicornContext};
use crate::extdev::sd::Response;
use crate::peripherals::aic::{InterruptNumber, post_interrupt};
use crate::{log_unsupported_read, log_unsupported_write};

pub const NAME_DMAC: &str = "DMAC";
pub const NAME_FMI: &str = "FMI";
pub const NAME_SD: &str = "SD";
pub const BASE: u64 = 0xB1006000;
pub const SIZE: usize = 0x1000;

pub const BASE_DMAC: u64 = 0x0;
pub const BASE_FMI: u64 = 0x800;

const REG_FB_0: u64 = BASE_DMAC;
const REG_FB_0_END: u64 = BASE_DMAC + 0x400;

const REG_DMACCSR: u64 = BASE_DMAC + 0x400;
const REG_DMACSAR: u64 = BASE_DMAC + 0x408;
const REG_DMACBCR: u64 = BASE_DMAC + 0x40c;
const REG_DMACIER: u64 = BASE_DMAC + 0x410;
const REG_DMACISR: u64 = BASE_DMAC + 0x414;

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
    dma_control: DMAControl,
    dma_dest_addr: u64,
    dma_irq_enable: DMAIRQFlags,
    dma_irq_status: DMAIRQFlags,
    dma_count: usize,
    fmi_control: FMIControl,
    sd_arg: u32,
    sd_response: (u32, u32),
    sd_control: SDCR,
    sd_irq_enable: SDIRQEnable,
    sd_irq: SDIRQStatus,
    sd_io_size: u64,
    fifo: [u8; 0x400],
    fmi_irq_enable: bool,
    fmi_irq_status: bool,
}

impl Default for SICConfig {
    fn default() -> Self {
        Self {
            dma_control: Default::default(),
            dma_dest_addr: Default::default(),
            dma_irq_enable: Default::default(),
            dma_irq_status: Default::default(),
            dma_count: Default::default(),
            fmi_control: Default::default(),
            sd_arg: Default::default(),
            sd_response: Default::default(),
            sd_control: Default::default(),
            sd_irq_enable: Default::default(),
            sd_irq: Default::default(),
            sd_io_size: 1u64,
            fifo: [0u8; 1024],
            fmi_irq_enable: Default::default(),
            fmi_irq_status: Default::default(),
        }
    }
}

#[bitfield]
#[derive(Default)]
struct DMAControl {
    enable: B1,
    reset: B1,
    scatter_gather_mode: B1,
    reserved_3: B6,
    busy: B1,
    reserved_10: B6,
}

#[bitfield]
#[derive(Default)]
struct FMIControl {
    reset: B1,
    sd_mode: B1,
    reserved_2: B1,
    nand_mode: B1,
    reserved_4: B4,
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

#[bitfield]
#[derive(Default)]
struct SDIRQEnable {
    block_xfer_done: B1,
    crc_error: B1,
    reserved_2: B6,
    card_detect: B1,
    reserved_9: B1,
    sdio: B1,
    reserved_11: B1,
    timeout_cmd: B1,
    timeout_dat: B1,
    wakeup: B1,
    reserved_15: B9,
    r1b: B1,
    reserved_25: B5,
    card_detect_mode: B1,  // 0 - Internal (uses DAT3 status), 1 - External (uses GPIO)
    reserved_31: B1,
}

#[bitfield]
#[derive(Default)]
struct SDIRQStatus {
    block_xfer_done: B1,
    crc_error: B1,
    crc_ok_cmd: B1,
    crc_ok_dat: B1,
    reserved_crc_ok: B3,
    available: B1,
    card_detect_changed: B1,
    reserved_9: B1,
    sdio: B1,
    reserved_11: B1,
    timeout_cmd: B1,
    timeout_dat: B1,
    reserved_14: B2,
    card_detect: B1,
    reserved_17: B1,
    data1: B1,
    reserved_19: B5,
    r1b: B1,
    reserved_25: B7,
}

#[bitfield]
#[derive(Default)]
struct DMAIRQFlags {
    target_abort: B1,
    wrong_eot: B1,
    reserved_2: B6,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if addr >= REG_FB_0 && addr < REG_FB_0_END {
        let fifo = &uc.get_data().sic.fifo;
        let fifo_addr: usize = (((addr - REG_FB_0) & 0x3ff) as u16).into();
        return match size {
            1 => fifo[fifo_addr].into(),
            2 => {
                if addr & 1 != 0 {
                    warn!("{NAME_DMAC}: Unaligned read16 at address {addr}.");
                    return 0;
                }
                u16::from_le_bytes(<[u8; 2]>::try_from(&fifo[fifo_addr..fifo_addr+2]).unwrap()).into()
            }
            4 => {
                if addr & 3 != 0 {
                    warn!("{NAME_DMAC}: Unaligned read32 at address {addr}.");
                    return 0;
                }
                u32::from_le_bytes(<[u8; 4]>::try_from(&fifo[fifo_addr..fifo_addr+4]).unwrap()).into()
            }
            _ => {
                log_unsupported_read!(addr, size);
                0
            }
        };
    }
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }
    let sic = &uc.get_data().sic;
    match addr {
        REG_DMACCSR => sic.dma_control.get(0, 16),
        REG_DMACSAR => sic.dma_dest_addr,
        REG_DMACBCR => u64::try_from(sic.dma_count & 0xffffffff).unwrap(),
        REG_DMACIER => sic.dma_irq_enable.get(0, 8),
        REG_DMACISR => sic.dma_irq_status.get(0, 8),
        REG_FMICR => sic.fmi_control.get(0, 8),
        REG_FMIIER => sic.fmi_irq_enable.into(),
        REG_FMIISR => sic.fmi_irq_status.into(),
        REG_SDCR => sic.sd_control.get(0, 32),
        REG_SDIER => sic.sd_irq_enable.get(0, 32),
        REG_SDISR => {
            let reg = sic.sd_irq.get(0, 32);
            trace!("Read REG_SDISR => 0x{reg:08x}");
            reg
        }
        REG_SDRSP0 => sic.sd_response.0.into(),
        REG_SDRSP1 => sic.sd_response.1.into(),
        REG_SDBLEN => (sic.sd_io_size - 1) & 0xffffffff,
        _ => {
            log_unsupported_read!(addr, size);
            0
        }
    }

}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    let data = uc.get_data_mut();

    if addr >= REG_FB_0 && addr < REG_FB_0_END {
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
            _ => log_unsupported_write!(addr, size, value),
        };
    }
    if size != 4 {
        log_unsupported_write!(addr, size, value);
        return;
    }
    let sic = &mut data.sic;
    match addr {
        REG_DMACCSR => sic.dma_control.set(0, 16, value),
        REG_DMACSAR => {
            trace!("DMA buffer address: 0x{value:08x}");
            sic.dma_dest_addr = value
        }
        REG_DMACBCR => sic.dma_count = usize::try_from(value & 0xffffffff).unwrap(),
        REG_DMACIER => sic.dma_irq_enable.set(0, 8, value),
        REG_DMACISR => sic.dma_irq_status.set(0, 8, value),
        REG_FMICR => sic.fmi_control.set(0, 8, value),
        REG_FMIIER => sic.fmi_irq_enable = value & 1 == 1,
        REG_FMIISR => sic.fmi_irq_status = value & 1 == 1,
        REG_SDARG => sic.sd_arg = value as u32,
        REG_SDCR => {
            let sd_control = &mut sic.sd_control;
            sd_control.set(0, 32, value);
        }
        REG_SDIER => sic.sd_irq_enable.set(0, 32, value),
        REG_SDISR => {
            trace!("Clear REG_SDISR mask=0x{value:08x}");
            let new_val = sic.sd_irq.get(0, 32) & !value & 0xffffffff;
            sic.sd_irq.set(0, 32, new_val);
        }
        REG_SDBLEN => sic.sd_io_size = (value + 1) & 0xffffffff,
        _ => log_unsupported_write!(addr, size, value),
    }
}

pub fn tick(uc: &mut UnicornContext, device: &mut Device) {
    // Do not tick if clock is disabled
    if uc.get_data().clk.ahbclk.get_sic() == 0 {
        return;
    }

    if check_reset(uc) || check_delay_condition(uc) {
        return;
    }

    let sd_control = &uc.get_data().sic.sd_control;
    let command_enable = sd_control.get_co_en() == 1;
    let sd_port = sd_control.get_sdport();
    let has_data_in = sd_control.get_di_en() == 1;
    let has_data_out = sd_control.get_do_en() == 1;

    let mut skip_data = false;

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
                    // TODO: Maybe make this a trait
                    Response::R1(response_type1) => {
                        sic_mut.sd_response = response_type1.into();
                        sic_mut.sd_control.set_ri_en(0);
                        sic_mut.sd_irq.set_crc_ok_cmd(1);
                    },
                    Response::R2(response_type2) => {
                        sic_mut.fifo[0] = 0b00111111;  // Needs to include header as well
                        sic_mut.fifo[1..response_type2.cid_csd.len()+1].copy_from_slice(&response_type2.cid_csd);
                        sic_mut.sd_control.set_r2_en(0);
                        sic_mut.sd_irq.set_crc_ok_cmd(1);
                    },
                    Response::R3(response_type3) => {
                        sic_mut.sd_response = response_type3.into();
                        sic_mut.sd_control.set_ri_en(0);
                        sic_mut.sd_irq.set_crc_ok_cmd(1);
                    },
                    Response::R6(response_type6) => {
                        sic_mut.sd_response = response_type6.into();
                        sic_mut.sd_control.set_ri_en(0);
                        sic_mut.sd_irq.set_crc_ok_cmd(1);
                    },
                    Response::R7(response_type7) => {
                        sic_mut.sd_response = response_type7.into();
                        sic_mut.sd_control.set_ri_en(0);
                        sic_mut.sd_irq.set_crc_ok_cmd(1);
                    },
                    Response::RNone => {
                        sic_mut.sd_irq.set_timeout_cmd(1);
                        // Skip any data transfer if pending
                        skip_data = true;
                        let has_data = has_data_in || has_data_out;
                        if has_data {
                            sic_mut.sd_control.set_di_en(0);
                            sic_mut.sd_control.set_do_en(0);
                            sic_mut.sd_irq.set_timeout_dat(1);
                        }

                        if sic_mut.sd_irq_enable.get_timeout_cmd() == 1 || (has_data && sic_mut.sd_irq_enable.get_timeout_dat() == 1) {
                            post_interrupt(uc, InterruptNumber::SIC, true, false);
                        }
                    },
                }
                let sic_mut = &mut uc.get_data_mut().sic;
                sic_mut.sd_control.set_co_en(0);
            }
            None => {
                warn!("Cannot send command through unmapped SD port {sd_port}");
            }
        }
    }

    if !skip_data && has_data_in {
        let dest = uc.get_data().sic.dma_dest_addr;
        let sd_device_op = match sd_port {
            0 => Some(&mut device.internal_sd),
            2 => Some(&mut device.external_sd),
            _ => None
        };

        match sd_device_op {
            Some(sd_device) => {
                let size = usize::try_from(uc.get_data().sic.sd_io_size).unwrap();
                let mult = usize::from(uc.get_data().sic.sd_control.get_blkcnt());
                // TODO multiply by blkcnt if that value is set, and then the recv_data routine will need to try to fill the slice as much as
                // possible until the limit on the SD card side has been reached.
                let size_final = if mult == 0 {
                    size
                } else {
                    size * mult
                };
                let mut buf = vec![0u8; size_final];
                sd_device.recv_data(&mut buf);
                match uc.mem_write(dest, &buf) {
                    Err(err) => {
                        error!("{NAME_DMAC}: Cannot write to 0x{dest:08x}: {err:?}");
                        uc.get_data_mut().sic.dma_irq_status.set_target_abort(1);
                        uc.get_data_mut().sic.sd_irq.set_crc_ok_dat(0);
                        if uc.get_data().sic.dma_irq_enable.get_target_abort() == 1 {
                            post_interrupt(uc, InterruptNumber::SIC, true, false);
                        }
                    },
                    Ok(_) => {
                        uc.get_data_mut().sic.dma_count += size_final;
                        uc.get_data_mut().sic.sd_irq.set_crc_ok_dat(1);
                        uc.get_data_mut().sic.sd_irq.set_block_xfer_done(1);
                        uc.get_data_mut().sic.dma_dest_addr += u64::try_from(size_final).unwrap();
                        if uc.get_data().sic.sd_irq_enable.get_block_xfer_done() == 1 {
                            post_interrupt(uc, InterruptNumber::SIC, true, false);
                        }
                    }
                }
                uc.get_data_mut().sic.sd_control.set_blkcnt(0);
            }
            None => {
                warn!("Cannot receive data through unmapped SD port {sd_port}");
            }
        }
        uc.get_data_mut().sic.sd_control.set_di_en(0);
    }

    if !skip_data && has_data_out {
        todo!();
    }
}

/// Handle reset condition.
pub fn check_reset(uc: &mut UnicornContext) -> bool {
    let mut has_reset = false;

    if uc.get_data().sic.dma_control.get_reset() == 1 {
        debug!("{NAME_DMAC}: Reset");
        // TODO: Reset callbacks go here.
        uc.get_data_mut().sic.dma_control.set_reset(0);
        has_reset = true;
    }

    if uc.get_data().sic.fmi_control.get_reset() == 1 {
        debug!("{NAME_FMI}: Reset");
        // TODO: Reset callbacks go here.
        uc.get_data_mut().sic.fmi_control.set_reset(0);
        has_reset = true;
    }

    if uc.get_data().sic.sd_control.get_swrst() == 1 {
        debug!("{NAME_SD}: Reset");
        // TODO: Reset callbacks go here.
        uc.get_data_mut().sic.sd_irq.set_available(1);
        uc.get_data_mut().sic.sd_control.set_swrst(0);
        has_reset = true;
    }

    return has_reset;
}

/// Handle SD card delay conditions
///
/// This is generally a no-op because we don't emulate SD card delays.
fn check_delay_condition(uc: &mut UnicornContext) -> bool {
    let sd_control = &mut uc.get_data_mut().sic.sd_control;
    if sd_control.get_clk74_oe() == 1 {
        trace!("SD delay 74 clock");
        sd_control.set_clk74_oe(0);
        true
    } else if sd_control.get_clk8_oe() == 1 {
        trace!("SD delay 8 clock");
        sd_control.set_clk8_oe(0);
        // HACK: Ensure DAT0 is high (card is available and not busy)
        // This needs to be changed once we have proper busy signaling (like from dedicated IO thread)
        uc.get_data_mut().sic.sd_irq.set_available(1);
        true
    } else {
        false
    }
}
