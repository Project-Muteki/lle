use log::warn;
use bit_field::{B2, B4, bitfield};

use crate::{device::{Device, UnicornContext}, extdev::input::{KeyPress, KeyType}, log_unsupported_read, log_unsupported_write, peripherals::aic::{InterruptNumber, post_interrupt}};

pub const BASE: u64 = 0xb8001000;
pub const SIZE: usize = 0x1000;

const REG_GPIO_BLOCK_START: u64 = 0x0;
const REG_GPIO_BLOCK_END: u64 = 0x50;
const REG_IRQSRC_BLOCK_START: u64 = 0x80;
const REG_IRQSRC_BLOCK_END: u64 = 0x94;
const REG_IRQEN_BLOCK_START: u64 = 0xa0;
const REG_IRQEN_BLOCK_END: u64 = 0xb4;
const REG_IRQLH_BLOCK_START: u64 = 0xd0;
const REG_IRQLH_BLOCK_END: u64 = 0xe4;

const REG_DBNCECON: u64 = 0x70;
const REG_IRQLHSEL: u64 = 0xc0;
const REG_IRQTGSRC0: u64 = 0xf0;
const REG_IRQTGSRC1: u64 = 0xf4;
const REG_IRQTGSRC2: u64 = 0xf8;

#[bitfield]
#[derive(Default)]
pub struct GPIOFlags {
    p0: bool,
    p1: bool,
    p2: bool,
    p3: bool,
    p4: bool,
    p5: bool,
    p6: bool,
    p7: bool,
    p8: bool,
    p9: bool,
    p10: bool,
    p11: bool,
    p12: bool,
    p13: bool,
    p14: bool,
    p15: bool,
}

#[bitfield]
#[derive(Default)]
pub struct GPIOIRQSource {
    p0: B2,
    p1: B2,
    p2: B2,
    p3: B2,
    p4: B2,
    p5: B2,
    p6: B2,
    p7: B2,
    p8: B2,
    p9: B2,
    p10: B2,
    p11: B2,
    p12: B2,
    p13: B2,
    p14: B2,
    p15: B2,
}

#[bitfield]
#[derive(Default)]
pub struct GPIODebounce {
    src_irq0: bool,
    src_irq1: bool,
    src_irq2: bool,
    src_irq3: bool,
    delay_power_of_2: B4,
}

#[bitfield]
#[derive(Default)]
pub struct GPIOIRQLatchSource {
    irq0: bool,
    irq1: bool,
    irq2: bool,
    irq3: bool,
    _reserved: B4,
}

#[derive(Default)]
pub struct GPIOChannel {
    pub output_mode: GPIOFlags,
    pub pull_up: GPIOFlags,
    pub data_out: GPIOFlags,
    pub data_in: GPIOFlags,
    pub irq_src: GPIOIRQSource,
    pub irq_enable: GPIOFlags,
    pub irq_latch: GPIOFlags,
    pub irq_trigger_source: GPIOFlags,
}

#[derive(Default)]
pub struct GPIOConfig {
    pub ports: [GPIOChannel; 5],
    pub debounce: GPIODebounce,
    pub irq_latch_source: GPIOIRQLatchSource,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }
    match addr {
        REG_GPIO_BLOCK_START..REG_GPIO_BLOCK_END => {
            let port = usize::from(((addr >> 4) & 0xf) as u8);
            let index = addr & 0xf;
            let port_obj = &uc.get_data().gpio.ports[port];
            match index {
                0x0 => port_obj.output_mode.get(0, 16).into(),
                0x4 => port_obj.pull_up.get(0, 16).into(),
                0x8 => port_obj.data_out.get(0, 16).into(),
                0xc => port_obj.data_in.get(0, 16).into(),
                _ => {
                    log_unsupported_read!(addr, size);
                    0
                },
            }
        }
        REG_IRQSRC_BLOCK_START..REG_IRQSRC_BLOCK_END => {
            let port = usize::from(((addr - REG_IRQSRC_BLOCK_START >> 4) & 0xf) as u8);
            uc.get_data().gpio.ports[port].irq_src.get(0, 32)
        }
        REG_IRQEN_BLOCK_START..REG_IRQEN_BLOCK_END => {
            let port = usize::from(((addr - REG_IRQEN_BLOCK_START >> 4) & 0xf) as u8);
            uc.get_data().gpio.ports[port].irq_enable.get(0, 16)
        }
        REG_IRQLH_BLOCK_START..REG_IRQLH_BLOCK_END => {
            let port = usize::from(((addr - REG_IRQLH_BLOCK_START >> 4) & 0xf) as u8);
            uc.get_data().gpio.ports[port].irq_latch.get(0, 16)
        }
        REG_DBNCECON => { uc.get_data().gpio.debounce.get(0, 8) }
        REG_IRQLHSEL => { uc.get_data().gpio.irq_latch_source.get(0, 4) }
        REG_IRQTGSRC0 => {
            let lo = uc.get_data().gpio.ports[0].irq_trigger_source.get(0, 16) as u32;
            let hi = uc.get_data().gpio.ports[1].irq_trigger_source.get(0, 16) as u32;
            ((hi << 16) | lo).into()
        }
        REG_IRQTGSRC1 => {
            let lo = uc.get_data().gpio.ports[2].irq_trigger_source.get(0, 16) as u32;
            let hi = uc.get_data().gpio.ports[3].irq_trigger_source.get(0, 16) as u32;
            ((hi << 16) | lo).into()
        }
        REG_IRQTGSRC2 => {
            uc.get_data().gpio.ports[4].irq_trigger_source.get(0, 16).into()
        }
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
        REG_GPIO_BLOCK_START..REG_GPIO_BLOCK_END => {
            let port = usize::from(((addr >> 4) & 0xf) as u8);
            let index = addr & 0xf;
            let port_obj = &mut uc.get_data_mut().gpio.ports[port];
            match index {
                0x0 => port_obj.output_mode.set(0, 16, value),
                0x4 => port_obj.pull_up.set(0, 16, value),
                0x8 => port_obj.data_out.set(0, 16, value),
                0xc => port_obj.data_in.set(0, 16, value),
                _ => {
                    log_unsupported_write!(addr, size, value);
                },
            }
        }
        REG_IRQSRC_BLOCK_START..REG_IRQSRC_BLOCK_END => {
            let port = usize::from(((addr - REG_IRQSRC_BLOCK_START >> 4) & 0xf) as u8);
            uc.get_data_mut().gpio.ports[port].irq_src.set(0, 32, value)
        }
        REG_IRQEN_BLOCK_START..REG_IRQEN_BLOCK_END => {
            let port = usize::from(((addr - REG_IRQEN_BLOCK_START >> 4) & 0xf) as u8);
            uc.get_data_mut().gpio.ports[port].irq_enable.set(0, 16, value)
        }
        REG_IRQLH_BLOCK_START..REG_IRQLH_BLOCK_END => {
            let port = usize::from(((addr - REG_IRQLH_BLOCK_START >> 4) & 0xf) as u8);
            uc.get_data_mut().gpio.ports[port].irq_latch.set(0, 16, value)
        }
        REG_DBNCECON => { uc.get_data_mut().gpio.debounce.set(0, 8, value) }
        REG_IRQTGSRC0 => {
            let gpio = &mut uc.get_data_mut().gpio;
            gpio.ports[0].irq_trigger_source.set(0, 16, value & 0xffff);
            gpio.ports[1].irq_trigger_source.set(0, 16, (value >> 16) & 0xffff);
        }
        REG_IRQTGSRC1 => {
            let gpio = &mut uc.get_data_mut().gpio;
            gpio.ports[2].irq_trigger_source.set(0, 16, value & 0xffff);
            gpio.ports[3].irq_trigger_source.set(0, 16, (value >> 16) & 0xffff);
        }
        REG_IRQTGSRC2 => {
            uc.get_data_mut().gpio.ports[4].irq_trigger_source.set(0, 16, value & 0xffff);
        }
        _ => {
            log_unsupported_write!(addr, size, value);
        }
    }
}

pub fn frame_step(uc: &mut UnicornContext, device: &mut Device) {
    if let Some(a) = device.input.check_key() {
        let gpio = &mut uc.get_data_mut().gpio;
        match a {
            KeyPress::Press(key_type) => {
                match key_type {
                    KeyType::Home => {
                        gpio.ports[0].data_in.set_p2(false);
                        gpio.ports[0].irq_latch.set_p2(true);
                    }
                    _ => {},
                }
            },
            KeyPress::Release(key_type) => {
                match key_type {
                    KeyType::Home => {
                        gpio.ports[0].data_in.set_p2(true);
                        gpio.ports[0].irq_latch.set_p2(true);
                    }
                    _ => {},
                }
                
            },
        }
        // TODO raise interrupt
    }
}
