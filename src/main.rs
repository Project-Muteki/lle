/// MMIO peripheral emulation routines.
mod peripherals;
/// External device emulation routines.
///
/// Normally this should not have direct access to the emulator states, and one should only exchange data between
/// the emulator context and routines defined under here.
mod extdev;
/// Device emulation context.
mod device;
/// CPU exception handling.
mod exception;

use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use log::info;
use unicorn_engine::ArmCpuModel;
use unicorn_engine::HookType;
use unicorn_engine::Permission;
use unicorn_engine::TlbType;
use unicorn_engine::Unicorn;
use unicorn_engine::Arch;
use unicorn_engine::Mode;
use unicorn_engine::uc_error;

use clap::Parser;
use env_logger;

use device::Device;
use peripherals::{sic, sys, gpio};

use crate::device::ExtraState;
use crate::device::UnicornContext;
use crate::peripherals::adc;
use crate::peripherals::aic;
use crate::peripherals::common::mmio_set_store_only;
use crate::peripherals::rtc;
use crate::peripherals::sdram;
use crate::peripherals::tmr;
use crate::peripherals::uart;

// TODO: Move this out of main
#[derive(Debug)]
pub enum RuntimeError {
    IOError(io::Error),
    UnicornError(uc_error),
    LoaderParserFailed,
    LoaderInvalidMagic,
    SDAlreadyMounted,
    SDNotMounted,
}

impl From<io::Error> for RuntimeError {
    fn from(value: io::Error) -> Self {
        Self::IOError(value)
    }
}

impl From<uc_error> for RuntimeError {
    fn from(value: uc_error) -> Self {
        Self::UnicornError(value)
    }
}

/// Nuvoton device emulator.
/// 
/// Emulates Nuvoton N329x-based devices made by Inventec Besta.
#[derive(Parser, Debug)]
#[command(version, about, long_about)]
struct Args {
    /// Embedded SD card image.
    #[arg(long)]
    esd: String,

    /// External SD card image.
    #[arg(long, required = false)]
    xsd: Option<String>,
}

#[inline]
fn read_le_u32(input: &[u8]) -> Result<u32, RuntimeError> {
    let conv = input.try_into().map_err(|_| RuntimeError::LoaderParserFailed)?;
    Ok(u32::from_le_bytes(conv))
}

/// Run HLE bootrom.
///
/// This initializes the emulator states and loads the first stage bootloader on the SD card image into the SDRAM region.
fn run_bootrom(uc: &mut UnicornContext, sd_image: &mut File) -> Result<(), RuntimeError> {
    let mut nvt_sd_boot_header: [u8; 32] = [0; 32];
    sd_image.seek(SeekFrom::Start(0x200))?;
    sd_image.read_exact(&mut nvt_sd_boot_header)?;

    let magic = read_le_u32(&nvt_sd_boot_header[0..4])?;
    let magic_tail = read_le_u32(&nvt_sd_boot_header[12..16])?;
    if magic != 0x57425aa5u32 || magic_tail != 0xa55a4257u32 {
        return Err(RuntimeError::LoaderInvalidMagic);
    }

    let load_addr = read_le_u32(&nvt_sd_boot_header[4..8])?;
    let load_size: usize = usize::try_from(read_le_u32(&nvt_sd_boot_header[8..12])?).map_err(|_| RuntimeError::LoaderParserFailed)?;
    info!("bootrom_hle: Loading 0x{load_size:x} bytes of code at 0x{load_addr:08x}...");

    let mut code = vec![0u8; load_size];
    sd_image.read_exact(&mut code)?;
    uc.mem_write(load_addr.into(), &code)?;
    uc.set_pc(load_addr.into())?;

    let config_clk = &mut uc.get_data_mut().clk;
    config_clk.ahbclk.set_cpu(true);
    config_clk.ahbclk.set_sram(true);

    // SDRAM stub
    mmio_set_store_only(uc, 0xb0003000, 0x00130456);
    mmio_set_store_only(uc, 0xb0003030, 0x00001010);
    mmio_set_store_only(uc, 0xb0003010, 0x00000005);
    mmio_set_store_only(uc, 0xb0003004, 0x00000021);
    mmio_set_store_only(uc, 0xb0003004, 0x00000023);
    mmio_set_store_only(uc, 0xb0003004, 0x00000027);
    mmio_set_store_only(uc, 0xb000301C, 0x00001002);
    mmio_set_store_only(uc, 0xb0003018, 0x00000122);
    mmio_set_store_only(uc, 0xb0003004, 0x00000027);
    mmio_set_store_only(uc, 0xb0003004, 0x0000002B);
    mmio_set_store_only(uc, 0xb0003004, 0x0000002B);
    mmio_set_store_only(uc, 0xb0003018, 0x00000022);
    mmio_set_store_only(uc, 0xb0003004, 0x00000020);
    mmio_set_store_only(uc, 0xb0003034, 0x00AAAA00);
    mmio_set_store_only(uc, 0xb0003008, 0x0000805A);
    mmio_set_store_only(uc, 0xb0003028, 0x094E7425);

    // Home Key - not pressed
    uc.get_data_mut().gpio.ports[0].data_in.set_p2(true);

    // VBAT comparator input
    uc.get_data_mut().gpio.ports[0].data_in.set_p3(true);

    // PCB Version (3)
    // TODO: Visually they look unconnected but actually measure these with a multimeter.
    uc.get_data_mut().gpio.ports[0].data_in.set_p0(true);
    uc.get_data_mut().gpio.ports[0].data_in.set_p7(true);

    // UPLL (192MHz)
    uc.get_data_mut().clk.apll.set_reg(0x0001c02e);
    uc.get_data_mut().clk.upll.set_reg(0x0000447e);
    uc.get_data_mut().clk.update_tick_config();

    // TODO: Move this to its own device with ticks and stuff (or at least it should be handled by ADC).
    uc.get_data_mut().adc.xdata = 1023;
    // TODO: Set other initial states

    info!("bootrom_hle: BootROM stage done.");
    Ok(())
}

/// Initialize emulator.
/// 
/// This does not populate registers, nor boots from the SD card. These are handled in run_bootrom().
fn emu_init<'a>() -> Result<UnicornContext<'a>, uc_error> {
    let mut uc = Unicorn::new_with_data(Arch::ARM, Mode::LITTLE_ENDIAN, ExtraState::default())?;
    uc.ctl_set_cpu_model(ArmCpuModel::UC_CPU_ARM_926.into())?;
    uc.ctl_tlb_type(TlbType::CPU)?;

    // Stop condition hook
    uc.add_code_hook(0, 0xffffffff, device::check_stop_condition)?;

    uc.add_mem_hook(HookType::MEM_UNMAPPED, 0, 0xffffffff, exception::unmapped_access)?;

    // MMIO registers
    uc.mmio_map(sys::BASE, sys::SIZE, Some(sys::read), Some(sys::write))?;
    uc.mmio_map(sdram::BASE, sdram::SIZE, Some(sdram::read), Some(sdram::write))?;
    uc.mmio_map(sic::BASE, sic::SIZE, Some(sic::read), Some(sic::write))?;
    uc.mmio_map(gpio::BASE, gpio::SIZE, Some(gpio::read), Some(gpio::write))?;
    uc.mmio_map(rtc::BASE, rtc::SIZE, Some(rtc::read), Some(rtc::write))?;
    uc.mmio_map(uart::BASE, uart::SIZE, Some(uart::read), Some(uart::write))?;
    uc.mmio_map(tmr::BASE, tmr::SIZE, Some(tmr::read), Some(tmr::write))?;
    uc.mmio_map(aic::BASE, aic::SIZE, Some(aic::read), Some(aic::write))?;
    uc.mmio_map(adc::BASE, adc::SIZE, Some(adc::read), Some(adc::write))?;

    // Memory
    // SDRAM (32MiB)
    uc.mem_map(0x80000000, 0x2000000, Permission::ALL)?;
    // SRAM (8KiB)
    uc.mem_map(0xff000000, 0x2000, Permission::ALL)?;

    Ok(uc)
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    let mut emulator = emu_init().unwrap();
    let mut device = Box::new(Device::default());
    let uc = &mut emulator;

    let mut esd_img = File::open(&args.esd).unwrap();
    run_bootrom(uc, &mut esd_img).unwrap();
    device.internal_sd.mount(&args.esd).unwrap();

    // TODO move this out of main
    loop {
        let pc = uc.pc_read().unwrap();
        uc.emu_start(pc, 0xffffffffffffffff, 0, 0).unwrap();
        if !device.tick(uc) {
            break;
        }
    }

    device.internal_sd.unmount();
    device.external_sd.unmount();
}
