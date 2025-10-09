use std::time::SystemTime;

use bit_field::{B4, B8, B12, bitfield};
use log::{debug, error, trace, warn};
use chrono::{DateTime, Datelike, Local, Timelike};

use crate::{device::{QuitDetail, StopReason, UnicornContext, request_stop}, log_unsupported_read, log_unsupported_write, peripherals::common::{mmio_get_store_only, mmio_set_store_only}};

pub const BASE: u64 = 0xb8003000;
pub const SIZE: usize = 0x1000;

const REG_INIR: u64 = 0x0;
const REG_AER: u64 = 0x4;
const REG_FCR: u64 = 0x8;
const REG_TLR: u64 = 0xc;
const REG_CLR: u64 = 0x10;
const REG_TSSR: u64 = 0x14;
const REG_DWR: u64 = 0x18;
const REG_PWRON: u64 = 0x34;

const MAGIC_INIT: u32 = 0xa5eb1357;
const MAGIC_WRITE: u16 = 0xa965;

#[derive(Default)]
pub struct RTCConfig {
    pub enabled: bool,
    pub write_enabled: bool,
    pub power_control: PowerControl,
    pub timekeeper: TimeKeeper,
}

#[bitfield]
pub struct PowerControl {
    power_on: bool,
    power_off: bool,
    power_off_delay_enable: bool,
    reserved_3: B4,
    power_key: bool,
    status: B8,
    power_off_delay_sec: B4,
    reserved_20: B12,
}

impl Default for PowerControl {
    fn default() -> Self {
        let mut result = Self::new();
        result.set_power_on(true);
        result
    }
}

pub struct TimeKeeper {
    pub is_24hr: bool,
    prev_sec: i64,
    cached_dt: DateTime<Local>,
}

impl Default for TimeKeeper {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeKeeper {
    pub fn new() -> Self {
        let (now, prev_sec) = Self::check_time();
        Self { is_24hr: Default::default(), prev_sec, cached_dt: DateTime::<Local>::from(now) }
    }

    pub fn get_time_reg(&self) -> u32 {
        let hour_bcd = if self.is_24hr {
            let hour = self.cached_dt.hour();
            (((hour / 10) << 4) | (hour % 10)) as u8
        } else {
            let (is_pm, hour) = self.cached_dt.hour12();
            (((hour / 10) << 4) | (hour % 10) | u32::from(is_pm) << 5) as u8
        };
        let minute = self.cached_dt.minute();
        let second = self.cached_dt.second();
        let minute_bcd = (((minute / 10) << 4) | (minute % 10)) as u8;
        let second_bcd = (((second / 10) << 4) | (second % 10)) as u8;
        u32::from_be_bytes([0, hour_bcd, minute_bcd, second_bcd])
    }

    pub fn get_date_reg(&self) -> u32 {
        let year = self.cached_dt.year() % 100;
        let month = self.cached_dt.month();
        let day = self.cached_dt.day();

        let year_bcd = (((year / 10) << 4) | (year % 10)) as u8;
        let month_bcd = (((month / 10) << 4) | (month % 10)) as u8;
        let day_bcd = (((day / 10) << 4) | (day % 10)) as u8;

        u32::from_be_bytes([0, year_bcd, month_bcd, day_bcd])
    }

    pub fn get_time_scale_reg(&self) -> u32 {
        u32::from(self.is_24hr)
    }

    pub fn get_day_of_week_reg(&self) -> u32 {
        let dow = self.cached_dt.weekday().num_days_from_sunday();
        u32::from(dow)
    }

    fn check_time() -> (SystemTime, i64) {
        let now = SystemTime::now();
        let current_sec = match now.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => (d.as_secs() & 0x7fffffffffffffff) as i64,
            Err(_err) => match SystemTime::UNIX_EPOCH.duration_since(now) {
                Ok(d) => -((d.as_secs() & 0x7fffffffffffffff) as i64),
                Err(_err) => {
                    error!("wtf");
                    0
                }
            }
        };
        (now, current_sec)
    }

    pub fn refresh(&mut self) {
        let (now, current_sec) = Self::check_time();
        if self.prev_sec != current_sec {
            trace!("Timestamp differs for 1 or more second. Refresh triggered.");
            self.prev_sec = current_sec;
            self.cached_dt = DateTime::<Local>::from(now);
        }
    }
}

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    uc.get_data_mut().rtc.timekeeper.refresh();

    match addr {
        REG_INIR => uc.get_data().rtc.enabled.into(),
        REG_AER => if uc.get_data().rtc.write_enabled { 0x10000 } else { 0x0 }
        REG_FCR => mmio_get_store_only(uc, BASE + addr),
        REG_TLR => uc.get_data().rtc.timekeeper.get_time_reg().into(),
        REG_CLR => uc.get_data().rtc.timekeeper.get_date_reg().into(),
        REG_TSSR => uc.get_data().rtc.timekeeper.get_time_scale_reg().into(),
        REG_DWR => uc.get_data().rtc.timekeeper.get_day_of_week_reg().into(),
        REG_PWRON => uc.get_data().rtc.power_control.get(0, 32),
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

    if addr == REG_AER && value == MAGIC_WRITE.into() {
        uc.get_data_mut().rtc.write_enabled = true;
        return;
    } else if !uc.get_data().rtc.write_enabled {
        warn!("Register 0x{addr:x} is write protected.");
        return;
    }

    match addr {
        REG_INIR => {
            if value == MAGIC_INIT.into() {
                debug!("MMIO reset.");
                // TODO reset
                uc.get_data_mut().rtc.enabled = true;
            }
        }
        REG_FCR => {
            debug!("Freq compensation: 0x{value:08x}");
            mmio_set_store_only(uc, BASE + addr, value);
        }
        REG_PWRON => {
            let power_control = &mut uc.get_data_mut().rtc.power_control;
            power_control.set(0, 32, value);
            if !power_control.get_power_on() || power_control.get_power_off() {
                debug!("RTC power off requested.");
                request_stop(uc, StopReason::Tick);
            }
        }
        // TODO Setting a time offset
        _ => {
            log_unsupported_write!(addr, size, value);
        }
    }

    //uc.get_data_mut().rtc.write_enabled = false;
}

pub fn tick(uc: &mut UnicornContext) {
    let power_control = &uc.get_data().rtc.power_control;
    if power_control.get_power_off() || !power_control.get_power_on() {
        uc.get_data_mut().stop_reason = StopReason::Quit(QuitDetail::CPUHalt);
    }
}
