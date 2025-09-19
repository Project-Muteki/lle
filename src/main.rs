mod peripherals;
mod extdev;
mod device;

use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use unicorn_engine::ArmCpuModel;
use unicorn_engine::Permission;
use unicorn_engine::Unicorn;
use unicorn_engine::Arch;
use unicorn_engine::Mode;
use unicorn_engine::uc_error;

use clap::Parser;
use env_logger;

use device::Device;
use peripherals::{sic, sys};

use crate::device::PeripheralState;
use crate::device::UnicornContext;

// TODO: Move this out of main
#[derive(Debug)]
pub enum RuntimeError {
    IOError(io::Error),
    UnicornError(uc_error),
    LoaderParserFailed,
    LoaderInvalidMagic,
    SDAlreadyMounted,
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
#[command(version, about, long_about = None)]
struct Args {
    /// Embedded SD card image.
    #[arg(long)]
    esd: String,

    /// External SD card image.
    #[arg(long, required = false)]
    xsd: Option<String>,
}

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

    let mut code = vec![0u8; load_size];
    sd_image.read_exact(&mut code)?;
    uc.mem_write(load_addr.into(), &code)?;
    uc.set_pc(load_addr.into())?;

    let config_clk = &mut uc.get_data_mut().clk;
    config_clk.ahbclk.set_cpu(1);
    config_clk.ahbclk.set_sram(1);

    Ok(())
}

/// Initialize emulator.
/// 
/// This does not populate registers, nor boots from the SD card. These are handled in run_bootrom().
fn emu_init<'a>() -> Result<UnicornContext<'a>, uc_error> {
    let mut uc = Unicorn::new_with_data(Arch::ARM, Mode::LITTLE_ENDIAN, PeripheralState::default())?;
    uc.ctl_set_cpu_model(ArmCpuModel::UC_CPU_ARM_926.into())?;

    // MMIO registers
    uc.mmio_map(sys::BASE, sys::SIZE, Some(sys::read), Some(sys::write))?;
    uc.mmio_map(sic::BASE, sic::SIZE, Some(sic::read), Some(sic::write))?;

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

    let mut esd_img = File::open(args.esd).unwrap();
    run_bootrom(uc, &mut esd_img).unwrap();

    // TODO move this out of main
    let pc = uc.pc_read().unwrap();

    loop {
        uc.emu_start(pc, 0xffffffffffffffff, 0, 0).unwrap();
        sic::tick(uc, device.as_mut());
    }
}
