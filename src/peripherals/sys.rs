use bit_field::{B1, B5, B6, B8, bitfield};
use log::info;

use crate::device::UnicornContext;
use crate::peripherals::common::{log_unsupported_read, log_unsupported_write, mmio_get_store_only, mmio_set_store_only};

pub const NAME: &str = "SYS";
pub const BASE: u64 = 0xB0000000;
pub const SIZE: usize = 0x1000;

const REG_CHIPID: u64 = 0x0;
const REG_CHIPCFG: u64 = 0x4;
const REG_SDRBIST: u64 = 0x24;
const REG_CRBIST: u64 = 0x28;
const REG_GPAFUN: u64 = 0x80;
const REG_GPBFUN: u64 = 0x84;
const REG_GPCFUN: u64 = 0x88;
const REG_GPDFUN: u64 = 0x8c;
const REG_GPEFUN: u64 = 0x90;

const CLK_BASE: u64 = 0x200;
const REG_PWRCON: u64 = CLK_BASE;
const REG_AHBCLK: u64 = CLK_BASE + 0x4;
const REG_APBCLK: u64 = CLK_BASE + 0x8;

#[bitfield]
#[derive(Default)]
pub struct AHBCLKRegister {
    cpu: B1,
    apbclk: B1,
    hclk: B1,
    sram: B1,
    dram: B1,
    blt: B1,
    fsc: B1,
    jpg: B1,

    hclk1: B1,
    ebi: B1,
    edma0: B1,
    edma1: B1,
    edma2: B1,
    edma3: B1,
    edma4: B1,
    des: B1,

    hclk3: B1,
    usbh: B1,
    usbd: B1,
    ge4p: B1,
    gpu: B1,
    sic: B1,
    nand: B1,
    sd: B1,

    hclk4: B1,
    spu: B1,
    i2s: B1,
    vpost: B1,
    cap: B1,
    sen: B1,
    ado: B1,
    _reserved_b31: B1,
}

#[bitfield]
#[derive(Default)]
pub struct APBCLKRegister {
    adc: B1,
    i2c: B1,
    rtc: B1,
    uart0: B1,
    uart1: B1,
    pwm: B1,
    spims0: B1,
    spims1: B1,

    tmr0: B1,
    tmr1: B1,
    _reserved_b10_14: B5,
    wdclk: B1,

    _reserved_b16_23: B8,

    tic: B1,
    kpi: B1,
    _reserved_b26_31: B6,
}


#[derive(Default)]
pub struct ClockConfig {
    pub ticks: u64,
    pub ahbclk: AHBCLKRegister,
    pub apbclk: APBCLKRegister,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read(NAME, addr, size);
        return 0;
    }

    match addr {
        REG_CHIPID => { 0x00fa5c30 }
        REG_CHIPCFG => { 0x00008020 }
        // Self-test should always return OK and not running.
        REG_SDRBIST | REG_CRBIST => { 0x00000000 }
        REG_AHBCLK => { uc.get_data().clk.ahbclk.get(0, 32) }
        REG_APBCLK => { uc.get_data().clk.apbclk.get(0, 32) }
        REG_GPAFUN | REG_GPBFUN | REG_GPCFUN | REG_GPDFUN | REG_GPEFUN => {
            mmio_get_store_only(uc, addr)
        }
        _ => {
            log_unsupported_read(NAME, addr, size);
            mmio_get_store_only(uc, addr)
        }
    }
}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    if size != 4 {
        log_unsupported_write(NAME, addr, size, value);
    }

    match addr {
        REG_AHBCLK => { uc.get_data_mut().clk.ahbclk.set(0, 32, value) }
        REG_APBCLK => { uc.get_data_mut().clk.apbclk.set(0, 32, value) }
        REG_GPAFUN => {
            info!("{NAME}: GPIOA config 0x{value:08x}");
            mmio_set_store_only(uc, addr, value);
        }
        REG_GPBFUN => {
            info!("{NAME}: GPIOB config 0x{value:08x}");
            mmio_set_store_only(uc, addr, value);
        }
        REG_GPCFUN => {
            info!("{NAME}: GPIOC config 0x{value:08x}");
            mmio_set_store_only(uc, addr, value);
        }
        REG_GPDFUN => {
            info!("{NAME}: GPIOD config 0x{value:08x}");
            mmio_set_store_only(uc, addr, value);
        }
        REG_GPEFUN => {
            info!("{NAME}: GPIOE config 0x{value:08x}");
            mmio_set_store_only(uc, addr, value);
        }
        _ => {
            log_unsupported_write(NAME, addr, size, value);
            mmio_set_store_only(uc, addr, value);
        }
    }
}
