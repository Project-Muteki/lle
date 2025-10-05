use log::{error, trace, warn};
use crate::{device::{Device, StopReason, UnicornContext, request_stop}, exception, log_unsupported_read, log_unsupported_write};

pub const BASE: u64 = 0xB8000000;
pub const SIZE: usize = 0x1000;

const REG_AIC_SCR_START: u64 = 0x0;
const REG_AIC_SCR_END: u64 = 0x20;
const REG_AIC_IRSR: u64 = 0x100;
const REG_AIC_IASR: u64 = 0x104;
const REG_AIC_ISR: u64 = 0x108;
const REG_AIC_IPER: u64 = 0x10c;
const REG_AIC_ISNR: u64 = 0x110;
const REG_AIC_IMR: u64 = 0x114;
const REG_AIC_OISR: u64 = 0x118;
const REG_AIC_MECR: u64 = 0x120;
const REG_AIC_MDCR: u64 = 0x124;
const REG_AIC_SSCR: u64 = 0x128;
const REG_AIC_SCCR: u64 = 0x12c;
const REG_AIC_EOSCR: u64 = 0x130;

const BCS8: [u8; 256] = [
    0, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    5, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    6, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    5, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    7, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    5, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    6, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    5, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
    4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0,
];

/// Flag storage and manipulation for AIC. Actual interrupt dispatch logic is in the `tick()` Device callback.
pub struct AICConfig {
    /// Raw level configuration.
    pub levels: [u32; 8],
    /// An index bitmap of each priority. Bit 1 means there are pending interrupts of this priority.
    pub status_map: u8,
    /// Interrupt state has changed since the last interrupt raise.
    pub step: bool,
    /// Status bitmap for each priority.
    pub status: [u32; 8],
    /// Interrupt mask bitmap (0 - masked, 1 - unmasked).
    pub enabled: u32,
    pub current_interrupt: (u8, u8),
}

impl Default for AICConfig {
    fn default() -> Self {
        Self {
            levels: [0x47474747; 8],
            status_map: Default::default(),
            step: Default::default(),
            status: Default::default(),
            enabled: Default::default(),
            current_interrupt: Default::default(),
        }
    }
}

#[allow(dead_code, reason = "For documentation purpose.")]
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum InterruptNumber {
    WDT = 1, EXTINT0, EXTINT1, EXTINT2, EXTINT3, SPU, I2S,  // 1..=7
    VPOST, VIDEOIN, GPU, BLT, FSC, HUART, TMR0, TMR1,  // 8..=15
    UDC, SIC, UHC, EDMA, SPIMS0, SPIMS1, ADC, RTC,  // 16..=23
    UART, PWM, JPG, PWM2, KPI, DES, I2C, PWR,  // 24..=31
}

impl Into<u8> for InterruptNumber {
    fn into(self) -> u8 {
        self as u8
    }
}

impl Into<usize> for InterruptNumber {
    fn into(self) -> usize {
        usize::from(self as u8)
    }
}

impl InterruptNumber {
    pub fn as_offset_shift(self) -> (usize, u8) {
        let a: usize = self.into();
        (a / 8, ((a % 8) * 8) as u8)
    }

    pub fn as_mask(self) -> u32 {
        1 << Into::<u8>::into(self)
    }
}

impl AICConfig {
    #[inline]
    fn get_level(&self, intno: InterruptNumber) -> u8 {
        let (offset, shift) = intno.as_offset_shift();
        u8::try_from((self.levels[offset] >> shift) & 0xff).unwrap()
    }

    /// Check whether there's a need to fire ann interrupt, and if so, record it and return `true`.
    pub fn check_interrupt(&mut self, intno: InterruptNumber, incoming: bool, latched: bool) -> bool {
        let mask: u32 = intno.as_mask();
        if self.enabled & mask != 0 {
            return false;
        }

        let level = self.get_level(intno);
        let trigger = match level & 0x30 {
            0x00 => !incoming,
            0x10 => incoming,
            0x20 => incoming != latched && !incoming,
            0x30 => incoming != latched && incoming,
            _ => panic!(),
        };

        if trigger {
            trace!("{intno:?} fired");

            let prio = level & 0x7;
            self.status[usize::from(prio)] |= mask;
            self.status_map |= 1 << (level & 0x7);

            return true;
            // Interrupt will then be caught in aic::tick()
        }
        false
    }

    /// Apply an enable mask to the interrupt mask register.
    pub fn apply_enable_mask(&mut self, mask: u32) {
        self.enabled |= mask;
    }

    /// Apply a disable mask to the interrupt mask register.
    pub fn apply_disable_mask(&mut self, mask: u32) {
        self.enabled &= !mask;
    }

    /// Flattern the priority table and output a single status bitfield.
    pub fn get_joint_status(&self) -> u32 {
        self.status.iter().fold(0, |acc, e| acc | e)
    }

    /// Inflate a joint status bitfield and replace the current priority table with the result.
    ///
    /// Note that this will not request a stop even when status is non-0.
    pub fn set_joint_status(&mut self, status: u32) {
        let mut new_status = [0u32; 8];
        let mut new_status_map = 0u8;
        for i in 0u8..32u8 {
            let mask = 1 << i;
            if status & mask != 0 {
                let offset = usize::from(i / 8);
                let shift = (i % 8) * 8;
                let level = u8::try_from((self.levels[offset] >> shift) & 0xff).unwrap();

                let prio = level & 0x7;
                new_status[usize::from(prio)] |= mask;
                new_status_map |= 1 << prio;
            }
        }
        self.status = new_status;
        self.status_map = new_status_map;
    }

    /// Apply a set mask to the joint status bitfield.
    pub fn apply_status_set_mask(&mut self, mask: u32) {
        let js = self.get_joint_status();
        self.set_joint_status(js | mask);
    }

    /// Apply a clear mask to the joint status bitfield.
    pub fn apply_status_clear_mask(&mut self, mask: u32) {
        let js = self.get_joint_status();
        self.set_joint_status(js & !mask);
    }

    pub fn next_interrupt(&self) -> (u8, u8) {
        if self.status_map == 0 {
            warn!("Interrupt status table is empty. This is probably a redundant check.");
            return (0, 0);
        }
        let next_pending_prio = BCS8[usize::from(self.status_map)];
        let next_pending = self.status[usize::from(next_pending_prio)];

        if next_pending == 0 {
            error!("Interrupt status table has bad index at prio {next_pending_prio}. This is a bug.");
            return (next_pending_prio, 0)
        }

        let mut num = 0;
        for i in 1..32 {
            if (1 << i) & next_pending != 0 {
                num = i;
                break;
            }
        };

        (next_pending_prio, num)
    }

    pub fn pop_next_interrupt(&mut self) -> (u8, u8) {
        let (prio, num) = self.next_interrupt();
        self.current_interrupt = (prio, num);
        let status = self.status[usize::from(prio)];
        self.status[usize::from(prio)] = status & !(1 << num);
        (prio, num)
    }
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    match addr {
        REG_AIC_SCR_START..REG_AIC_SCR_END => {
            if addr % 4 != 0 {
                log_unsupported_read!(addr, size);
                return 0;
            }

            uc.get_data().aic.levels[usize::try_from(addr / 4).unwrap()].into()
        }
        REG_AIC_IPER => u64::from(uc.get_data().aic.current_interrupt.1) << 2,
        REG_AIC_ISNR => uc.get_data().aic.current_interrupt.1.into(),
        REG_AIC_IMR => uc.get_data().aic.enabled.into(),
        REG_AIC_ISR => {
            uc.get_data_mut().aic.step = false;
            uc.get_data().aic.get_joint_status().into()
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
    }

    let v32 = u32::try_from(value & 0xffffffff).unwrap();
    match addr {
        REG_AIC_SCR_START..REG_AIC_SCR_END => {
            if addr % 4 != 0 {
                log_unsupported_write!(addr, size, value);
            }

            uc.get_data_mut().aic.levels[usize::try_from(addr / 4).unwrap()] = v32;
        }
        REG_AIC_IMR => uc.get_data_mut().aic.enabled = v32,
        REG_AIC_ISR => {
            uc.get_data_mut().aic.set_joint_status(v32);
            if v32 != 0 {
                uc.get_data_mut().aic.step = true;
                request_stop(uc, StopReason::Tick);
            }
        }
        REG_AIC_MECR => uc.get_data_mut().aic.apply_enable_mask(v32),
        REG_AIC_MDCR => uc.get_data_mut().aic.apply_disable_mask(v32),
        REG_AIC_SSCR => {
            uc.get_data_mut().aic.apply_status_set_mask(v32);
            if v32 != 0 {
                uc.get_data_mut().aic.step = true;
                request_stop(uc, StopReason::Tick);
            }
        }
        REG_AIC_SCCR => {
            uc.get_data_mut().aic.apply_status_clear_mask(v32);
            // Clear is guaranteed to not trigger an interrupt, so no request_stop() here.
        }
        REG_AIC_EOSCR => {
            // Request stop so aic::tick() can dispatch the next interrupt.
            if uc.get_data().aic.get_joint_status() != 0 {
                uc.get_data_mut().aic.step = true;
                request_stop(uc, StopReason::Tick);
            }
        }
        _ => log_unsupported_write!(addr, size, value),
    }
    
}

pub fn tick(uc: &mut UnicornContext, _device: &mut Device) {
    if uc.get_data().aic.step && uc.get_data().aic.status_map != 0 {
        let (prio, _) = uc.get_data_mut().aic.pop_next_interrupt();
        exception::call_exception_handler(uc, match prio {
            0 => exception::ExceptionType::FIQ,
            _ => exception::ExceptionType::IRQ,
        }).unwrap_or_else(|err| {
            error!("Failed to invoke exception handler: {err:?}.");
        });
    }
}

/// Utility function for the host part of the emulator to inject an interrupt.
///
/// This will automatically initiate an emulator stop when necessary.
#[inline]
pub fn post_interrupt(uc: &mut UnicornContext, intno: InterruptNumber, incoming: bool, latched: bool) {
    if uc.get_data_mut().aic.check_interrupt(intno, incoming, latched) {
        uc.get_data_mut().aic.step = true;
        request_stop(uc, StopReason::Tick);
    }
}
