use std::mem;

use bit_field::{B2, B3, B6, bitfield};
use log::{info, warn};

use crate::{device::UnicornContext, log_unsupported_read, log_unsupported_write};

pub const BASE: u64 = 0xb8008000;
pub const SIZE: usize = 0x1000;

pub const REG_UART_DATA: u64 = 0x0;
pub const REG_UART_IER: u64 = 0x4;
pub const REG_UART_FCR: u64 = 0x8;
pub const REG_UART_LCR: u64 = 0xc;
pub const REG_UART_MCR: u64 = 0x10;
pub const REG_UART_MSR: u64 = 0x14;
pub const REG_UART_FSR: u64 = 0x18;
pub const REG_UART_ISR: u64 = 0x1c;
pub const REG_UART_TOR: u64 = 0x20;
pub const REG_UART_BAUD: u64 = 0x24;

#[derive(Default)]
pub struct UARTConfig {
    ports: [UARTPort; 2],
}

pub struct UARTPort {
    fifo_status: UARTFIFOStatus,
    line_buffer: [u8; 80],
    line_offset: usize,
}

impl Default for UARTPort {
    fn default() -> Self {
        let mut fifo_status = UARTFIFOStatus::new();
        fifo_status.set_rx_empty(true);
        fifo_status.set_tx_empty(true);
        Self { fifo_status, line_buffer: [0u8; 80], line_offset: 0 }
    }
}

#[bitfield]
#[derive(Default)]
pub struct UARTFIFOStatus {
    rx_overflow: bool,
    reserved_1: B3,
    parity_error: bool,
    framing_error: bool,
    break_int: bool,
    rx_err: bool,
    rx_pointer: B6,
    rx_empty: bool,
    rx_full: bool,
    tx_pointer: B6,
    tx_empty: bool,
    tx_full: bool,
    tx_overflow: bool,
    reserved_26: B3,
    te_flag: bool,
    reserved_29: B2,
    tx_err: bool,
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    let port = usize::from(((addr >> 8) & 0x1) as u8);
    let paddr = addr & 0xff;

    match size {
        // 1 => {
        //     TODO: support inject data into UART
        // }
        4 => {
            match paddr {
                REG_UART_FSR => uc.get_data().uart.ports[port].fifo_status.get(0, 32),
                _ => {
                    log_unsupported_read!(addr, size);
                    0
                }
            }
        }
        _ => {
            log_unsupported_read!(addr, size);
            0
        }
    }


}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    //log_unsupported_write!(addr, size, value);
    let port = usize::from(((addr >> 8) & 0x1) as u8);
    let paddr = addr & 0xff;

    match size {
        1 => if paddr == REG_UART_DATA {
            let port_obj = &mut uc.get_data_mut().uart.ports[port];
            port_obj.line_buffer[port_obj.line_offset] = value as u8;
            port_obj.line_offset += 1;
            if port_obj.line_offset == port_obj.line_buffer.len() || value == 0x0a {
                let line_buffer = mem::replace(&mut port_obj.line_buffer, [0u8; 80]);
                let printable = String::from_utf8_lossy(&line_buffer[..port_obj.line_offset]);
                info!("UART{port}: {}", printable.trim());
                port_obj.line_offset = 0;
            }
        } else {
            log_unsupported_write!(addr, size, value);
        },
        4 => match paddr {
            _ => log_unsupported_write!(addr, size, value),
        },
        _ => log_unsupported_write!(addr, size, value),
    }
}
