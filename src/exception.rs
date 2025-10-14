use std::{fs::File, io::Write};

use log::{error, trace};
use unicorn_engine::{MemType, RegisterARM, uc_error};

use crate::{RuntimeError, device::{QuitDetail, StopReason, UnicornContext, request_stop}};

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
        ExceptionType::SupervisorCall => current_pc,  // Next instruction (QEMU already gives us next instruction)
        ExceptionType::PrefetchAbort => current_pc + 4,  // Affected instruction + 4
        ExceptionType::DataAbort => current_pc + 8,  // Affected instruction + 8
        ExceptionType::IRQ => current_pc + 4,  // Next instruction
        ExceptionType::FIQ => current_pc + 4,  // Next instruction
    };

    let computed_cpsr_set = match exc_type {
        ExceptionType::Reset => 0b11010011,  // svc, no interrupt
        ExceptionType::UndefinedInstruction => 0b10011011,  // und, no irq
        ExceptionType::SupervisorCall => 0b10010011,  // svc, no irq
        ExceptionType::PrefetchAbort | ExceptionType::DataAbort => 0b10010111,  // abt, no irq
        ExceptionType::IRQ => 0b10010010,  // irq, no irq
        ExceptionType::FIQ => 0b11010001,  // fiq, no interrupt
    };

    let cpsr = uc.reg_read(RegisterARM::CPSR)?;
    let new_cpsr = (cpsr & !0b00111111) | computed_cpsr_set;
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
        request_stop(uc, StopReason::SVC);
    } else {
        error!("Not int2. This should not have happened.");
        request_stop(uc, StopReason::Quit(QuitDetail::CPUException));
    }
}

pub fn dump_data(uc: &UnicornContext) -> Result<(), RuntimeError> {
    let regs: Vec<u64> = uc.reg_read_batch(&[
        RegisterARM::R0,
        RegisterARM::R1,
        RegisterARM::R2,
        RegisterARM::R3,
        RegisterARM::R4,
        RegisterARM::R5,
        RegisterARM::R6,
        RegisterARM::R7,
        RegisterARM::R8,
        RegisterARM::R9,
        RegisterARM::R10,
        RegisterARM::R11,
        RegisterARM::R12,
        RegisterARM::SP,
        RegisterARM::LR,
        RegisterARM::PC,
        RegisterARM::CPSR,
        RegisterARM::SPSR,
    ], 18)?.iter().map(|val| val & 0xffffffff).collect();
    error!("R0=0x{:08x} R1=0x{:08x} R2=0x{:08x} R3=0x{:08x}", regs[0], regs[1], regs[2], regs[3]);
    error!("R4=0x{:08x} R5=0x{:08x} R6=0x{:08x} R7=0x{:08x}", regs[4], regs[5], regs[6], regs[7]);
    error!("R8=0x{:08x} R9=0x{:08x} R10=0x{:08x} R11=0x{:08x}", regs[8], regs[9], regs[10], regs[11]);
    error!("R12=0x{:08x} SP=0x{:08x} LR=0x{:08x} PC=0x{:08x}", regs[12], regs[13], regs[14], regs[15]);
    error!("CPSR=0x{:08x} SPSR=0x{:08x}", regs[16], regs[17]);
    let mut sdram_dump = File::options().write(true).create(true).open("sdram.bin")?;
    sdram_dump.write(&uc.get_data().raw_sdram)?;
    let mut sram_dump = File::options().write(true).create(true).open("sram.bin")?;
    let sram_data = uc.mem_read_as_vec(0xff000000, 8192)?;
    sram_dump.write(&sram_data)?;
    Ok(())
}
