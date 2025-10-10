use log::{error, trace};
use unicorn_engine::{MemType, RegisterARM, uc_error};

use crate::device::{QuitDetail, StopReason, UnicornContext, request_stop};

#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum ExceptionType {
    Reset = 0x0,
    UndefinedInstruction = 0x4,
    SupervisorCall = 0x8,
    PrefetchAbort = 0xc,
    DataAbort = 0x10,
    IRQ = 0x18,
    FIQ = 0x1c,
}

impl ExceptionType {
    #[inline]
    pub fn to_vector_address(self) -> u64 {
        // N3290x likely keeps the exception handler trampolines in bootrom, which is mapped at where the high
        // exception handlers are normally at.
        // We don't emulate the bootrom so the exception handlers will be mapped at 0xff000000 instead.
        0xff000000u64 + (self as u64)
    }
}

pub fn call_exception_handler(uc: &mut UnicornContext, exc_type: ExceptionType) -> Result<(), uc_error> {
    /* Notes on PC:
     * - Exceptions will leave the PC at the unexecuted instruction.
     * - IRQ and FIQ that are triggered by MMIO are usually triggered after the instruction that generated it, but
     *   before that instruction is executed.
     * In all cases, current_pc will be the resume point.
     */
    let current_pc = uc.pc_read()?;
    
    // TODO properly compute offset of next instruction
    let computed_lr = match exc_type {
        ExceptionType::Reset => current_pc,  // Undefined
        ExceptionType::UndefinedInstruction => current_pc + 4,  // Next instruction
        ExceptionType::SupervisorCall => current_pc + 4,  // Next instruction
        ExceptionType::PrefetchAbort => current_pc + 4,  // Affected instruction + 4
        ExceptionType::DataAbort => current_pc + 8,  // Affected instruction + 8
        ExceptionType::IRQ => current_pc + 4,  // Next instruction
        ExceptionType::FIQ => current_pc + 4,  // Next instruction
    };

    let computed_mode = match exc_type {
        ExceptionType::Reset => 0b11111,  // sys
        ExceptionType::UndefinedInstruction => 0b11011,  // und
        ExceptionType::SupervisorCall => 0b10011,  // svc
        ExceptionType::PrefetchAbort | ExceptionType::DataAbort => 0b10111,  // abt
        ExceptionType::IRQ => 0b10010,  // irq
        ExceptionType::FIQ => 0b10001,  // fiq
    };

    let cpsr = uc.reg_read(RegisterARM::CPSR)?;
    let new_cpsr = (cpsr & !0b11111) | computed_mode;
    // Switch mode
    uc.reg_write(RegisterARM::CPSR, new_cpsr)?;
    uc.reg_write(RegisterARM::SPSR, cpsr)?;
    uc.reg_write(RegisterARM::LR, computed_lr)?;
    uc.set_pc(exc_type.to_vector_address())?;
    trace!("Exception {exc_type:?} raised @ 0x{current_pc:08x}");
    Ok(())
}

pub fn unmapped_access(uc: &mut UnicornContext, access_type: MemType, addr: u64, size: usize, value: i64) -> bool {
    let pc = uc.pc_read().unwrap();
    error!("exception: {access_type:?} of {size} bytes at 0x{addr:08x}, value 0x{value:08x}, by 0x{pc:08x}.");
    false
}

pub fn intr(uc: &mut UnicornContext, intno: u32) {
    if intno == 2 {
        let pc = uc.pc_read().unwrap();
        uc.reg_write(RegisterARM::LR, pc).unwrap();
        request_stop(uc, StopReason::SVC);
    } else {
        error!("Not int2. This should not have happened.");
        request_stop(uc, StopReason::Quit(QuitDetail::CPUException));
    }
}
