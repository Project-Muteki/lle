use log::warn;

use crate::device::UnicornContext;

#[inline(always)]
pub fn log_unsupported_read(context: &str, addr: u64, size: usize) {
    let size_bits = size * 8;
    warn!("{context}: Unsupported read{size_bits} @ 0x{addr:08x}");
}

#[inline(always)]
pub fn log_unsupported_write(context: &str, addr: u64, size: usize, value: u64) {
    let size_bits = size * 8;
    warn!("{context}: Unsupported write{size_bits} of {value:08x} @ 0x{addr:08x}");
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
