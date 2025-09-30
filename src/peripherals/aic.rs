use log::{error, trace, warn};
use crate::{device::{Device, StopReason, UnicornContext, request_stop}, log_unsupported_read, log_unsupported_write};

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

#[derive(Default)]
pub struct AICConfig {
    /// Raw level configuration.
    pub levels: [u32; 8],
    /// An index bitmap of each priority. Bit 1 means there are pending interrupts of this priority.
    pub status_map: u8,
    /// Status bitmap for each priority.
    pub status: [u32; 8],
    /// Interrupt mask bitmap (0 - masked, 1 - unmasked).
    pub enabled: u32,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum InterruptNumber {
    WDT = 1,
    EXTINT0,
    EXTINT1,
    EXTINT2,
    EXTINT3,
    SPU,
    I2S,
    VPOST,
    VIDEOIN,
    GPU,
    BLT,
    FSC,
    HUART,
    TMR0,
    TMR1,
    UDC,
    SIC,
    UHC,
    EDMA,
    SPIMS0,
    SPIMS1,
    ADC,
    RTC,
    UART,
    PWM,
    JPG,
    PWM2,
    KPI,
    DES,
    I2C,
    PWR,
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

    /// Utility method for the host part of the emulator to inject an interrupt.
    ///
    /// This will automatically initiate an emulator stop when necessary, unlike other register manipulation methods.
    pub fn post_interrupt(&mut self, uc: &mut UnicornContext, intno: InterruptNumber, incoming: bool, latched: bool) {
        let mask: u32 = intno.as_mask();
        if self.enabled & mask != 0 {
            return;
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

            request_stop(uc, StopReason::AIC);
            // Interrupt will then be caught in aic::tick()
        }
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
        REG_AIC_IMR => uc.get_data().aic.enabled.into(),
        REG_AIC_ISR => uc.get_data().aic.get_joint_status().into(),
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
                request_stop(uc, StopReason::AIC);
            }
        }
        REG_AIC_MECR => uc.get_data_mut().aic.apply_enable_mask(v32),
        REG_AIC_MDCR => uc.get_data_mut().aic.apply_disable_mask(v32),
        REG_AIC_SSCR => {
            uc.get_data_mut().aic.apply_status_set_mask(v32);
            if v32 != 0 {
                request_stop(uc, StopReason::AIC);
            }
        }
        REG_AIC_SCCR => {
            uc.get_data_mut().aic.apply_status_clear_mask(v32);
            // Clear is guaranteed to not trigger an interrupt, so no request_stop() here.
        }
        REG_AIC_EOSCR => {
            // Request stop so aic::tick() can dispatch the next interrupt.
            if value == 1 && uc.get_data().aic.get_joint_status() != 0 {
                request_stop(uc, StopReason::AIC);
            }
        }
        _ => log_unsupported_write!(addr, size, value),
    }
    
}

pub fn tick(_uc: &mut UnicornContext, _device: &mut Device) {

}
