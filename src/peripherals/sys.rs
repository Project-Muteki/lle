use std::fmt::Display;

use bit_field::{B1, B2, B3, B4, B5, B6, B7, B8, bitfield};
use log::{info, warn, debug};

use crate::{log_unsupported_read, log_unsupported_write};
use crate::device::{Device, QuitDetail, StopReason, UnicornContext, request_stop};
use crate::peripherals::common::{mmio_get_store_only, mmio_set_store_only};

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
const REG_CLKDIV0: u64 = CLK_BASE + 0xc;
const REG_CLKDIV1: u64 = CLK_BASE + 0x10;
const REG_CLKDIV2: u64 = CLK_BASE + 0x14;
const REG_CLKDIV3: u64 = CLK_BASE + 0x18;
const REG_CLKDIV4: u64 = CLK_BASE + 0x1c;
const REG_APLLCON: u64 = CLK_BASE + 0x20;
const REG_UPLLCON: u64 = CLK_BASE + 0x24;

pub const F_BASE: u64 = 12_000_000;

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

#[bitfield]
#[derive(Default, Debug, PartialEq)]
pub enum ClockSource {
    #[default]
    XIN,
    X32K,
    APLL,
    UPLL,
}

#[bitfield]
#[derive(Default, Debug, PartialEq)]
pub enum ClockSource1B {
    #[default]
    XIN,
    X32K,
}

#[bitfield]
#[derive(Default)]
pub struct ClockDivider0 {
    sys_prediv: B3,
    sys_source: ClockSource,
    kpi_source: ClockSource1B,
    reserved_6: B2,
    sys_div: B4,
    kpi_prediv: B4,
    sensor_prediv: B3,
    sensor_source: ClockSource,
    kpi_div: B3,
    sensor_div: B4,
    reserved_28: B4,
}

#[bitfield]
#[derive(Default)]
pub struct ClockDivider1 {
    vpost_prediv: B3,
    vpost_source: ClockSource,
    reserved_5: B3,
    vpost_div: B8,

    ado_prediv: B3,
    ado_source: ClockSource,
    reserved_21: B3,
    ado_div: B8,
}

#[bitfield]
#[derive(Default)]
pub struct ClockDivider2 {
    usb_prediv: B3,
    usb_source: ClockSource,
    reserved_5: B3,
    usb_div: B4,
    reserved_12: B4,

    sd_prediv: B3,
    sd_source: ClockSource,
    reserved_21: B3,
    sd_div: B8,
}

#[bitfield]
#[derive(Default)]
pub struct ClockDivider3 {
    uart0_prediv: B3,
    uart0_source: ClockSource,
    uart0_div: B3,

    uart1_prediv: B3,
    uart1_source: ClockSource,
    uart1_div: B3,

    adc_prediv: B3,
    adc_source: ClockSource,
    reserved_21: B3,
    adc_div: B8,
}

#[bitfield]
#[derive(Default)]
pub struct ClockDivider4 {
    cpu_div: B4,
    hclk_div: B4,
    apb_div: B4,
    cap_div: B3,
    reserved_15: B1,
    gpio_source: ClockSource1B,
    gpio_div: B7,
    jpg_div: B3,
    reserved_27: B5,
}

#[derive(Default)]
pub struct ClockConfig {
    pub ahbclk: AHBCLKRegister,
    pub apbclk: APBCLKRegister,
    pub apll: PLLConfig,
    pub upll: PLLConfig,
    pub clkdiv0: ClockDivider0,
    pub clkdiv1: ClockDivider1,
    pub clkdiv2: ClockDivider2,
    pub clkdiv3: ClockDivider3,
    pub clkdiv4: ClockDivider4,
}

#[derive(Default)]
pub struct PLLConfig {
    reg: u64,
    fout: u64,
}

impl Display for PLLConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PLLConfig(reg=0x{:08x}, fout={})", self.reg, self.fout)?;
        Ok(())
    }
}

impl PLLConfig {
    #[inline]
    pub fn get_reg(&self) -> u64 {
        self.reg
    }

    #[inline]
    pub fn get_fout(&self) -> u64 {
        self.fout
    }

    pub fn set_reg(&mut self, value: u64) {
        self.reg = value;
        self.fout = calculate_pll_fout(value.into());
    }
}

fn calculate_pll_fout(pll: u64) -> u64 {
    const OUT_DV_VALS: [u64; 4] = [1, 2, 2, 4];
    let fb = pll & 0b1_1111_1111;
    let in_dv = (pll >> 9) & 0b1_1111;
    let out_dv = usize::try_from((pll >> 14) & 0b11).unwrap();
    ((F_BASE / 1000) * (2 * (fb + 2)) / (2 * (in_dv + 2)) / (OUT_DV_VALS[out_dv])) * 1000
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    match addr {
        REG_CHIPID => { 0x00fa5c30 }
        REG_CHIPCFG => { 0x0003077b }
        // Self-test should always return OK and not running.
        REG_SDRBIST | REG_CRBIST => { 0x00000000 }
        REG_AHBCLK => uc.get_data().clk.ahbclk.get(0, 32),
        REG_APBCLK => uc.get_data().clk.apbclk.get(0, 32),
        REG_CLKDIV0 => uc.get_data().clk.clkdiv0.get(0, 32),
        REG_CLKDIV1 => uc.get_data().clk.clkdiv1.get(0, 32),
        REG_CLKDIV2 => uc.get_data().clk.clkdiv2.get(0, 32),
        REG_CLKDIV3 => uc.get_data().clk.clkdiv3.get(0, 32),
        REG_CLKDIV4 => uc.get_data().clk.clkdiv4.get(0, 32),
        REG_GPAFUN | REG_GPBFUN | REG_GPCFUN | REG_GPDFUN | REG_GPEFUN => {
            mmio_get_store_only(uc, BASE + addr)
        }
        REG_APLLCON => uc.get_data().clk.apll.get_reg(),
        REG_UPLLCON => uc.get_data().clk.upll.get_reg(),
        _ => {
            log_unsupported_read!(addr, size);
            mmio_get_store_only(uc, BASE + addr)
        }
    }
}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    if size != 4 {
        log_unsupported_write!(addr, size, value);
    }

    match addr {
        REG_AHBCLK => {
            uc.get_data_mut().clk.ahbclk.set(0, 32, value);
            // AHBCLK may halt the CPU. Request a tick.
            request_stop(uc, StopReason::Tick);
        }
        REG_APBCLK => { uc.get_data_mut().clk.apbclk.set(0, 32, value) }
        REG_CLKDIV0 => {
            uc.get_data_mut().clk.clkdiv0.set(0, 32, value);
        }
        REG_CLKDIV1 => {
            uc.get_data_mut().clk.clkdiv1.set(0, 32, value);
        }
        REG_CLKDIV2 => {
            uc.get_data_mut().clk.clkdiv2.set(0, 32, value);
        }
        REG_CLKDIV3 => {
            uc.get_data_mut().clk.clkdiv3.set(0, 32, value);
        }
        REG_CLKDIV4 => {
            uc.get_data_mut().clk.clkdiv4.set(0, 32, value);
        }
        REG_GPAFUN => {
            debug!("GPIOA config 0x{value:08x}");
            mmio_set_store_only(uc, BASE + addr, value);
        }
        REG_GPBFUN => {
            debug!("GPIOB config 0x{value:08x}");
            mmio_set_store_only(uc, BASE + addr, value);
        }
        REG_GPCFUN => {
            debug!("GPIOC config 0x{value:08x}");
            mmio_set_store_only(uc, BASE + addr, value);
        }
        REG_GPDFUN => {
            debug!("GPIOD config 0x{value:08x}");
            mmio_set_store_only(uc, BASE + addr, value);
        }
        REG_GPEFUN => {
            debug!("GPIOE config 0x{value:08x}");
            mmio_set_store_only(uc, BASE + addr, value);
        }
        REG_APLLCON => {
            uc.get_data_mut().clk.apll.set_reg(value);
            debug!("Config APLL with {}", uc.get_data().clk.apll);
        }
        REG_UPLLCON => {
            uc.get_data_mut().clk.upll.set_reg(value);
            debug!("Config UPLL with {}", uc.get_data().clk.upll);
        }
        _ => {
            log_unsupported_write!(addr, size, value);
            mmio_set_store_only(uc, BASE + addr, value);
        }
    }
}

pub fn tick(uc: &mut UnicornContext, _device: &mut Device) {
    if uc.get_data().clk.ahbclk.get_cpu() == 0 {
        uc.get_data_mut().stop_reason = StopReason::Quit(QuitDetail::CPUHalt);
    }
}


#[test]
fn test_calculate_pll_fout() {
    assert_eq!(calculate_pll_fout(0x0000001e), 192_000_000);
}
