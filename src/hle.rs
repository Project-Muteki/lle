use log::error;

use crate::{RuntimeError, device::{QuitDetail, UnicornContext, request_stop}};

fn printf(uc: &mut UnicornContext) -> Result<(), RuntimeError> {
    Ok(())
}

pub fn printf_callback(uc: &mut UnicornContext, _addr: u64, _size: u32) {
    printf(uc).unwrap_or_else(|err| {
        error!("Failed to execute printf: {err:?}");
        request_stop(uc, crate::device::StopReason::Quit(QuitDetail::HLECallbackFailure));
    })
}
