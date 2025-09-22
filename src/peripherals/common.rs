
use crate::device::UnicornContext;

#[macro_export]
macro_rules! log_unsupported_read {
    ($addr:expr, $size:expr) => {
        warn!("Unsupported read{} @ 0x{:08x}", 8 * $size, $addr)
    };
}

#[macro_export]
macro_rules! log_unsupported_write {
    ($addr:expr, $size:expr, $value:expr) => {
        warn!("Unsupported write{} of value 0x{:08x} @ 0x{:08x}", 8 * $size, $addr, $value)
    };
}

#[inline]
pub fn mmio_get_store_only(uc: &mut UnicornContext, addr: u64) -> u64 {
    match uc.get_data().store_only.get(&addr) {
        None => 0u64,
        Some(&v) => v,
    }
}

#[inline]
pub fn mmio_set_store_only(uc: &mut UnicornContext, addr: u64, value: u64) {
    uc.get_data_mut().store_only.insert(addr, value);
}
