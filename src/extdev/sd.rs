
use std::fmt::Display;
use std::fs;
use std::os::unix::fs::MetadataExt;

use bit_field::B1;
use bit_field::B2;
use bit_field::B3;
use bit_field::B4;
use bit_field::B5;
use bit_field::B6;
use bit_field::B7;
use bit_field::B8;
use bit_field::B12;
use bit_field::B22;
use bit_field::bitfield;
use log::debug;
use log::warn;

use crate::RuntimeError;

/*
Commands directly used by BSP:

CMD0
CMD1 (CMD55 unsupported -> MMC)
CMD2
CMD3
CMD6
CMD7
CMD8
CMD9
CMD10
CMD12
CMD16
CMD18
CMD25
CMD55
    ACMD6 (?)
    ACMD41
    ACMD51
*/

const CID_ESD: [u8; 16] = [0x00, 0x45, 0x6d, 0x49, 0x6e, 0x74, 0x53, 0x44, 0x10, 0xde, 0xad, 0xbe, 0xef, 0x00, 0xe1, 0x6f];
const CID_XSD: [u8; 16] = [0x00, 0x45, 0x6d, 0x45, 0x78, 0x74, 0x53, 0x44, 0x10, 0xde, 0xad, 0xbe, 0xef, 0x00, 0xe1, 0x65];
const SCR_ESD: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

const SDSC_MAX_CAPACITY: u64 = 0x80000000;

#[bitfield]
#[derive(Default, Debug, PartialEq)]
pub enum CurrentState {
    #[default]
    Idle,
    Ready,
    Identification,
    StandBy,
    Transfer,
    SendingData,
    ReceivingData,
    Programming,
    Disabled,
    Inactive,
    Reserved10,
    Reserved11,
    Reserved12,
    Reserved13,
    Reserved14,
    Reserved15,
}

#[must_use]
pub enum Response {
    RNone,
    R1(ResponseType1),
    R2(ResponseType2),
    R3(ResponseType3),
    R6(ResponseType6),
    R7(ResponseType7),
}

pub struct ResponseType1 {
    pub cmd: u8,
    pub status: CardStatus,
    pub busy: bool,
}

pub struct ResponseType2 {
    pub cid_csd: [u8; 16],
}

pub struct ResponseType3 {
    pub ocr: u32,
    pub is_sdhc: bool,
    pub power_up: bool,
}

pub struct ResponseType6 {
    pub rca: u16,
    pub status: CardStatus,
}

pub struct ResponseType7 {
    pub voltage_accepted: u8,
    pub check: u8,
}

impl Into<(u32, u32)> for ResponseType1 {
    fn into(self) -> (u32, u32) {
        let reg0 = ((u32::from(self.cmd) & 0b111111) << 24) | (self.status.get(8, 24) as u32);
        let reg1 = self.status.get(0, 8) as u32;
        (reg0, reg1)
    }
}

impl Into<(u32, u32)> for ResponseType2 {
    fn into(self) -> (u32, u32) {
        // TODO The BSP seems to ignore these. Do we need to put anything here?
        (0, 0)
    }
}

impl Into<(u32, u32)> for ResponseType3 {
    fn into(self) -> (u32, u32) {
        let reg0 = (0b00111111 << 24) | (u32::from(self.power_up) << 23) | (u32::from(self.is_sdhc) << 22) | (self.ocr >> 8);
        (reg0, self.ocr & 0xff)
    }
}

impl Into<(u32, u32)> for ResponseType6 {
    fn into(self) -> (u32, u32) {
        let short_status = (
            (self.status.get(22, 2) << 14) |
            (u64::from(self.status.get_bit(19)) << 13) |
            self.status.get(0, 13)
        ) as u32;
        let reg0 = (0b00000011u32 << 24) | u32::from(self.rca) << 8 | ((short_status & 0xff00) >> 8);
        (reg0, short_status & 0xff)
    }
}

impl Into<(u32, u32)> for ResponseType7 {
    fn into(self) -> (u32, u32) {
        let reg0 = (0b00001000u32 << 24) | u32::from(self.voltage_accepted);
        (reg0, self.check.into())
    }
}

#[bitfield]
#[derive(Default, Copy, Clone)]
pub struct CardStatus {
    test: B2,  // 0..=1
    app_specific: B1,  // 2
    ake_seq_error: B1,  // 3
    reserved_sdio: B1,  // 4
    app_command: B1,  // 5
    function_event: B1,  // 6
    reserved_7: B1,  // 7
    ready_for_data: B1,  // 8
    current_state: CurrentState,  // 9..=12
    erased_reset: B1,  // 13
    card_ecc_disabled: B1,  // 14
    wp_erase_skip: B1,  // 15
    csd_overwrite: B1,  // 16
    reserved_deferred_response: B1,  // 17
    reserved_18: B1,  // 18
    general_error: B1,  // 19
    controller_error: B1,  // 20
    card_ecc_failed: B1,  // 21
    illegal_command: B1,  // 22
    command_crc_error: B1,  // 23
    lock_unlock_failed: B1,  // 24
    card_is_locked: B1,  // 25
    wp_violation: B1,  // 26
    erase_param: B1,  // 27
    erase_seq_error: B1,  // 28
    block_len_error: B1,  // 29
    address_error: B1,  // 30
    out_of_range: B1,  // 31
}

impl CardStatus {
    pub fn after_read(&mut self) -> CardStatus {
        let before_clear = self.clone();
        self.set(26, 6, 0);
        self.set_lock_unlock_failed(0);
        self.set(19, 3, 0);
        self.set(15, 2, 0);
        self.set_erased_reset(0);
        self.set_app_command(0);
        self.set_ake_seq_error(0);
        before_clear
    }
}

#[derive(Default, Debug)]
pub enum SendAction {
    #[default]
    None,
    FTLWrite{sector_index: u64, sector_count: u64},
}

#[derive(Default, Debug)]
pub enum RecvAction {
    #[default]
    None,
    FTLRead{sector_index: u64, sector_count: u64},
    SDHCPowerProfileRead,
}

#[bitfield]
pub struct CardSpecificSC {
    tail: B1,
    crc: B7,

    reserved_8: B2,
    file_format: B2,
    tmp_write_protect: B1,
    perm_write_protect: B1,
    copy: B1,
    file_format_grp: B1,

    reserved_16: B5,
    write_bl_partial: B1,
    write_bl_len: B4,
    r2w_factor: B3,

    reserved_29: B2,
    wp_grp_enable: B1,
    wp_grp_size: B7,
    sector_size: B7,
    erase_blk_en: B1,
    c_size_mult: B3,
    vdd_w_curr_max: B3,
    vdd_w_curr_min: B3,
    vdd_r_curr_max: B3,
    vdd_r_curr_min: B3,
    c_size: B12,

    reserved_74: B2,
    dsr_imp: B1,
    read_blk_misalign: B1,
    write_blk_misalign: B1,
    read_bl_partial: B1,
    read_bl_len: B4,
    ccc: B12,
    tran_speed: B8,
    nsac: B8,
    taac: B8,

    reserved_120: B6,
    csd_structure: B2,
}

impl Default for CardSpecificSC {
    fn default() -> Self {
        let mut result = Self::new();
        result.set_csd_structure(0);
        result.set_tail(1);
        result.set_tran_speed(0x032);  // 25MHz
        result.set_ccc(0x5b5);
        result.set_read_bl_partial(1);
        result.set_write_bl_partial(1);
        result.set_read_bl_len(0xa);  // 1024 bytes
        result.set_write_bl_len(0xa);  // 1024 bytes
        result.set_c_size_mult(0x7);  // 512x
        result.set_r2w_factor(0x2);  // 4x
        result.set_erase_blk_en(1);
        result.set_sector_size(0x7f);  // 64KiB
        result.set_vdd_r_curr_min(0x7);
        result.set_vdd_r_curr_max(0x7);
        result.set_vdd_w_curr_min(0x7);
        result.set_vdd_w_curr_max(0x7);
        result
    }
}

#[bitfield]
#[derive(Clone)]
pub struct CardSpecificHC {
    tail: B1,
    crc: B7,

    reserved_8: B2,
    file_format: B2,
    tmp_write_protect: B1,
    perm_write_protect: B1,
    copy: B1,
    file_format_grp: B1,

    reserved_16: B5,
    write_bl_partial: B1,
    write_bl_len: B4,
    r2w_factor: B3,

    reserved_29: B2,
    wp_grp_enable: B1,
    wp_grp_size: B7,
    sector_size: B7,
    erase_blk_en: B1,

    reserved_47: B1,
    c_size: B22,

    reserved_70: B6,
    dsr_imp: B1,
    read_blk_misalign: B1,
    write_blk_misalign: B1,
    read_bl_partial: B1,
    read_bl_len: B4,
    ccc: B12,
    tran_speed: B8,
    nsac: B8,
    taac: B8,

    reserved_120: B6,
    csd_structure: B2,
}

impl Default for CardSpecificHC {
    fn default() -> Self {
        let mut result = Self::new();
        result.set_csd_structure(1);
        result.set_tail(1);
        result.set_taac(0xe);  // 1ms
        result.set_tran_speed(0x032);  // 25MHz
        result.set_ccc(0x5b5);
        result.set_read_bl_len(0x9);  // 512 bytes
        result.set_write_bl_len(0x9);  // 512 bytes
        result.set_r2w_factor(0x2);  // 4x
        result.set_erase_blk_en(1);
        result.set_sector_size(0x7f);  // 64KiB
        result
    }
}

pub enum CardSpecific {
    SC(CardSpecificSC),
    HC(CardSpecificHC),
}

impl Display for CardSpecific {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytestream: [u8; 16] = self.as_bytes();
        for b in bytestream {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl CardSpecific {
    pub fn is_sdhc(&self) -> bool {
        match self {
            Self::SC(_) => false,
            Self::HC(_) => true,
        }
    }

    pub fn init_with_size(size: u64) -> Self {
        // TODO this cannot be unwrap
        if size > SDSC_MAX_CAPACITY {
            let mut result = CardSpecificHC::default();
            result.set_c_size(u32::try_from(size / 1024 / 512).unwrap() - 1);
            Self::HC(result)
        } else {
            let mut result = CardSpecificSC::default();
            result.set_c_size(u16::try_from(size / 1024 / 512).unwrap() - 1);
            Self::SC(result)
        }
    }

    pub fn as_bytes(&self) -> [u8; 16] {
        match self {
            CardSpecific::SC(csd) => {
                ((u128::from(csd.get(64, 64)) << 64) | u128::from(csd.get(0, 64))).to_be_bytes()
            },
            CardSpecific::HC(csd) => {
                ((u128::from(csd.get(64, 64)) << 64) | u128::from(csd.get(0, 64))).to_be_bytes()
            },
        }
    }
}

// fn crc7(data: &[u8]) -> u8 {
//     const CRC7_POLY: u8 = 0x89;

//     let mut crc = 0u8;
//     data.iter().for_each(|byte| {
//         crc ^= byte;
//         for _ in 0..8 {
//             crc = if crc & 0x80 == 0x80 {
//                 (crc << 1) ^ (CRC7_POLY << 1)
//             } else {
//                 crc << 1
//             }
//         }
//     });
//     crc >> 1
// }

// #[test]
// fn test_crc7() {
//     const TEST_VEC: [u8; 15] = [0x40, 0x0e, 0x00, 0x32, 0x5b, 0x59, 0x00, 0x00, 0xed, 0x9f, 0x7f, 0x80, 0x0a, 0x40, 0x00];
//     const TEST_VEC_2: [u8; 15] = [0x40, 0x0e, 0x00, 0x32, 0x5b, 0x59, 0x00, 0x00, 0xef, 0x37, 0x7f, 0x80, 0x0a, 0x40, 0x00];
//     assert_eq!(crc7(&TEST_VEC), 0x49);
//     assert_eq!(crc7(&TEST_VEC_2), 0x12);
// }

#[derive(Default)]
pub struct SD {
    csd: Option<CardSpecific>,
    card_status: CardStatus,
    rca: u16,
    image_file: Option<fs::File>,
    send_action: SendAction,
    recv_action: RecvAction,
}

impl SD {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn mount(&mut self, path: &str) -> Result<(), RuntimeError> {
        if self.image_file.is_some() {
            return Err(RuntimeError::SDAlreadyMounted)
        }
        let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
        self.image_file = Some(file);
        let size = self.image_file.as_ref().unwrap().metadata()?.size();

        let csd_inner = CardSpecific::init_with_size(size);
        debug!("Emulated CSD: {}", &csd_inner);

        self.csd = Some(csd_inner);

        self.send_action = SendAction::None;
        self.recv_action = RecvAction::None;

        Ok(())
    }

    pub fn unmount(&mut self) {
        self.image_file = None;
        self.csd = None;
    }

    /// Make a request on the CMD channel.
    pub fn make_request(&mut self, cmd: u8, arg: u32) -> Response {
        if self.card_status.get_app_command() == 1 {
            // ACMD
            // TODO
            return match cmd {
                41 => {
                    if self.card_status.get_current_state() == CurrentState::Idle {
                        let is_sdhc = match &self.csd {
                            Some(csdd) => csdd.is_sdhc(),
                            None => false,
                        };
                        if arg & 0x00ffffff == 0 {
                            debug!("ACMD41 query");
                            self.card_status.after_read();
                            Response::R3(ResponseType3 { ocr: 0x00ffff00, is_sdhc, power_up: false })
                        } else {
                            debug!("ACMD41 set arg=0x{arg:08x}");
                            self.card_status.after_read();
                            self.card_status.set_current_state(CurrentState::Ready);
                            Response::R3(ResponseType3 { ocr: arg & 0x00ffffff, is_sdhc, power_up: true })
                        }

                    } else {
                        self.term_illegal()
                    }
                }
                _ => {
                    self.term_illegal()
                },
            };
        }
        match cmd {
            0 => {
                self.card_status.set(0, 32, 0u64);
                self.rca = 0;
                Response::R1(ResponseType1 { cmd, status: self.card_status, busy: false })
            }
            2 | 10 => {
                if self.card_status.get_current_state() == CurrentState::Ready {
                    self.card_status.set_current_state(CurrentState::Identification);
                    Response::R2(ResponseType2 { cid_csd: CID_ESD.clone() })
                } else {
                    self.term_illegal()
                }
            }
            3 => {
                match self.card_status.get_current_state() {
                    CurrentState::Identification | CurrentState::StandBy => {
                        self.card_status.set_current_state(CurrentState::StandBy);
                        self.rca = 1;
                        Response::R6(ResponseType6 { rca: self.rca, status: self.card_status })
                    }
                    _ => self.term_illegal()
                }
            }
            6 => {
                if self.card_status.get_current_state() == CurrentState::Transfer {
                    self.recv_action = RecvAction::SDHCPowerProfileRead;
                    self.card_status.set_current_state(CurrentState::SendingData);
                    Response::R1(ResponseType1 { cmd, status: self.card_status, busy: false })
                } else {
                    self.term_illegal()
                }
            }
            8 => {
                if self.card_status.get_current_state() == CurrentState::Idle {
                    Response::R7(ResponseType7 {
                        voltage_accepted: u8::try_from((arg >> 8) & 0xf).unwrap(),
                        check: u8::try_from(arg & 0xff).unwrap(),
                    })
                } else {
                    self.term_illegal()
                }
            }
            9 => {
                let rca = u16::try_from(arg >> 16).unwrap();
                if self.card_status.get_current_state() == CurrentState::StandBy && self.rca == rca {
                    debug!("Read CSD RCA={rca}");
                    match &self.csd {
                        None => self.term_illegal(),
                        Some(csdd) => Response::R2(ResponseType2 { cid_csd: csdd.as_bytes() })
                    }
                } else {
                    self.term_illegal()
                }
            }
            55 => {
                debug!("CMD55");
                let status = self.card_status.after_read();
                self.card_status.set_app_command(1);
                Response::R1(ResponseType1 { cmd, status, busy: false })
            }
            _ => {
                // Stuck at 0x80a00c84
                warn!("Unhandled SD card command {cmd}");
                self.term_illegal()
            }
        }
    }

    /// Send data to the emulated SD card through the DAT channel.
    pub fn send_data(&mut self, data: &[u8]) {

    }

    /// Receive data from the emulated SD card through the DAT channel.
    pub fn recv_data(&mut self, data: &mut [u8]) {

    }

    /// Set the `ILLEGAL_COMMAND` status bit and respond with a no response. Should always use with a return.
    #[inline(always)]
    fn term_illegal(&mut self) -> Response {
        self.card_status.set_illegal_command(1);
        Response::RNone
    }
}
