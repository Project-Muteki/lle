use std::io::{Read, Seek, SeekFrom};
use std::{fmt::Display};
use std::fs;
use std::os::unix::fs::MetadataExt;

use bit_field::{B1, B2, B3, B4, B5, B6, B7, B8, B12, B22, bitfield};
use log::{debug, error, trace, warn};

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
    ACMD6
    ACMD41
    ACMD51
*/

const CID_ESD: [u8; 16] = [0x00, 0x45, 0x6d, 0x49, 0x6e, 0x74, 0x53, 0x44, 0x10, 0xde, 0xad, 0xbe, 0xef, 0x00, 0xe1, 0x6f];
const CID_XSD: [u8; 16] = [0x00, 0x45, 0x6d, 0x45, 0x78, 0x74, 0x53, 0x44, 0x10, 0xde, 0xad, 0xbe, 0xef, 0x00, 0xe1, 0x65];
// SD spec V2.00, erases to 0, no security, 1-and-4-bit interface, no optional command support.
const SCR: [u8; 8] = [0x02, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
// From 6 to 1
const CARD_FUNC: [u16; 6] = [
    0b1000000000000001,  // Reserved
    0b1000000000000001,  // Reserved
    0b1000000000000001,  // 0.72W
    0b1000000000000001,  // Type B
    0b1000000000000001,  // Default
    0b1000000000000011,  // SDR12, SDR25
];

const SDSC_MAX_CAPACITY: u64 = 0x80000000;

#[bitfield]
#[derive(Default, Debug, PartialEq)]
pub enum CurrentState {
    /// Card is partially powered on and waiting for power mode selection.
    #[default]
    Idle,
    /// Card is fully powered on and waiting for address configuration.
    Ready,
    /// Card has identified itself and waiting for further address configuration.
    Identification,
    /// Card is configured but unselected.
    StandBy,
    /// Card is configured and selected.
    Transfer,
    /// Card is sending data to the host (host is receiving data from the card).
    SendingData,
    /// Card is receiving data from the host (host is sending data to the card).
    ReceivingData,
    /// Card is selected by the host and is writing data to the storage backend.
    Programming,
    /// Card is not selected by the host and is writing data to the storage backend. No status reporting will be done unless it's reselected.
    Disconnect,
    /// Card is powered off by the host. It will not process any further commands unless the host power-cycles it.
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

#[bitfield]
#[derive(Default, Copy, Clone)]
pub struct CardStatus {
    test: B2,  // 0..=1
    app_specific: bool,  // 2
    ake_seq_error: bool,  // 3
    reserved_sdio: bool,  // 4
    app_command: bool,  // 5
    function_event: bool,  // 6
    reserved_7: B1,  // 7
    ready_for_data: bool,  // 8
    current_state: CurrentState,  // 9..=12
    erased_reset: bool,  // 13
    card_ecc_disabled: bool,  // 14
    wp_erase_skip: bool,  // 15
    csd_overwrite: bool,  // 16
    reserved_deferred_response: bool,  // 17
    reserved_18: B1,  // 18
    general_error: bool,  // 19
    controller_error: bool,  // 20
    card_ecc_failed: bool,  // 21
    illegal_command: bool,  // 22
    command_crc_error: bool,  // 23
    lock_unlock_failed: bool,  // 24
    card_is_locked: bool,  // 25
    wp_violation: bool,  // 26
    erase_param: bool,  // 27
    erase_seq_error: bool,  // 28
    block_len_error: bool,  // 29
    address_error: bool,  // 30
    out_of_range: bool,  // 31
}

impl CardStatus {
    pub fn after_read(&mut self) -> CardStatus {
        // TODO cross-check with table 4-43
        let before_clear = self.clone();
        self.set(26, 6, 0);
        self.set_lock_unlock_failed(false);
        self.set(19, 3, 0);
        self.set(15, 2, 0);
        self.set_erased_reset(false);
        self.set_app_command(false);
        self.set_ake_seq_error(false);
        before_clear
    }
}

#[derive(Default, Debug)]
pub enum SendAction {
    #[default]
    None,
    FTLWrite{sector_index: u64},
}

#[derive(Default, Debug)]
pub enum RecvAction {
    #[default]
    None,
    FTLRead{sector_index: u64},
    SCRRead,
    FunctionStatus{ arg: u32 },
}

#[bitfield]
pub struct CardSpecificSC {
    tail: B1,
    crc: B7,

    reserved_8: B2,
    file_format: B2,
    tmp_write_protect: bool,
    perm_write_protect: bool,
    copy: bool,
    file_format_grp: B1,

    reserved_16: B5,
    write_bl_partial: bool,
    write_bl_len: B4,
    r2w_factor: B3,

    reserved_29: B2,
    wp_grp_enable: bool,
    wp_grp_size: B7,
    sector_size: B7,
    erase_blk_en: bool,
    c_size_mult: B3,
    vdd_w_curr_max: B3,
    vdd_w_curr_min: B3,
    vdd_r_curr_max: B3,
    vdd_r_curr_min: B3,
    c_size: B12,

    reserved_74: B2,
    dsr_imp: bool,
    read_blk_misalign: bool,
    write_blk_misalign: bool,
    read_bl_partial: bool,
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
        result.set_read_bl_partial(true);
        result.set_write_bl_partial(true);
        result.set_read_bl_len(0xa);  // 1024 bytes
        result.set_write_bl_len(0xa);  // 1024 bytes
        result.set_c_size_mult(0x7);  // 512x
        result.set_r2w_factor(0x2);  // 4x
        result.set_erase_blk_en(true);
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
    tmp_write_protect: bool,
    perm_write_protect: bool,
    copy: bool,
    file_format_grp: B1,

    reserved_16: B5,
    write_bl_partial: bool,
    write_bl_len: B4,
    r2w_factor: B3,

    reserved_29: B2,
    wp_grp_enable: bool,
    wp_grp_size: B7,
    sector_size: B7,
    erase_blk_en: bool,

    reserved_47: B1,
    c_size: B22,

    reserved_70: B6,
    dsr_imp: bool,
    read_blk_misalign: bool,
    write_blk_misalign: bool,
    read_bl_partial: bool,
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
        result.set_erase_blk_en(true);
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

#[derive(Default)]
pub struct SD {
    csd: Option<CardSpecific>,
    card_status: CardStatus,
    rca: u16,
    selected_functions: u32,
    io_size: u32,
    image_file: Option<fs::File>,
    send_action: SendAction,
    recv_action: RecvAction,
}

impl SD {
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

    pub fn is_mounted(&self) -> bool {
        self.image_file.is_some()
    }

    /// Make a request on the CMD channel.
    pub fn make_request(&mut self, cmd: u8, arg: u32) -> Response {
        if !self.is_mounted() {
            return Response::RNone;
        }
        if self.card_status.get_app_command() {
            // ACMD
            // TODO
            trace!("ACMD{cmd} arg=0x{arg:08x}");
            return match cmd {
                6 => {
                    if self.card_status.get_current_state() == CurrentState::Transfer {
                        debug!("arg=0x{arg:08x}");
                        let status = self.card_status.after_read();
                        Response::R1(ResponseType1 { cmd, status, busy: false })
                    } else {
                        self.term_illegal()
                    }
                }
                41 => {
                    if self.card_status.get_current_state() == CurrentState::Idle {
                        let is_sdhc = match &self.csd {
                            Some(csdd) => csdd.is_sdhc(),
                            None => false,
                        };
                        if arg & 0x00ffffff == 0 {
                            debug!("query");
                            self.card_status.after_read();
                            Response::R3(ResponseType3 { ocr: 0x00ffff00, is_sdhc, power_up: false })
                        } else {
                            debug!("set arg=0x{arg:08x}");
                            self.card_status.set_current_state(CurrentState::Ready);
                            self.card_status.after_read();
                            Response::R3(ResponseType3 { ocr: arg & 0x00ffffff, is_sdhc, power_up: true })
                        }

                    } else {
                        self.term_illegal()
                    }
                }
                51 => {
                    if self.card_status.get_current_state() == CurrentState::Transfer {
                        self.recv_action = RecvAction::SCRRead;
                        self.card_status.set_current_state(CurrentState::SendingData);
                        let status = self.card_status.after_read();
                        Response::R1(ResponseType1 { cmd, status, busy: false })
                    } else {
                        self.term_illegal()
                    }
                }
                _ => {
                    warn!("Unhandled SD card application command {cmd}");
                    self.term_illegal()
                },
            };
        }
        trace!("CMD{cmd} arg=0x{arg:08x}");
        match cmd {
            0 => {
                self.card_status.set(0, 32, 0u64);
                self.rca = 0;
                Response::R1(ResponseType1 { cmd, status: self.card_status, busy: false })
            }
            2 => {
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
                        let status = self.card_status.after_read();
                        self.rca = 1;
                        Response::R6(ResponseType6 { rca: self.rca, status })
                    }
                    _ => self.term_illegal()
                }
            }
            6 => {
                // See 4.3.10
                if self.card_status.get_current_state() == CurrentState::Transfer {
                    self.recv_action = RecvAction::FunctionStatus{arg};
                    self.card_status.set_current_state(CurrentState::SendingData);
                    let status = self.card_status.after_read();
                    Response::R1(ResponseType1 { cmd, status, busy: false })
                } else {
                    self.term_illegal()
                }
            }
            7 => {
                let rca = u16::try_from(arg >> 16).unwrap();
                match self.card_status.get_current_state() {
                    CurrentState::StandBy => {
                        if rca == self.rca {
                            // StandBy -> Transfer when addressed.
                            trace!("select RCA={}", self.rca);
                            self.card_status.set_current_state(CurrentState::Transfer);
                        }
                        // StandBy -> StandBy when NOT addressed.
                        let status = self.card_status.after_read();
                        Response::R1(ResponseType1 { cmd, status, busy: false })
                    }
                    CurrentState::Transfer | CurrentState::SendingData => {
                        if rca != self.rca {
                            // {Transfer, SendingData} -> StandBy when NOT addressed.
                            trace!("deselect RCA={}", self.rca);
                            self.card_status.set_current_state(CurrentState::StandBy);
                            let status = self.card_status.after_read();
                            Response::R1(ResponseType1 { cmd, status, busy: false })
                        } else {
                            // Illegal when card is already in Transfer state but is being selected.
                            warn!("Cannot select RCA={rca} as it is already been selected.");
                            self.term_illegal()
                        }
                    }
                    _ => {
                        warn!("Invalid select");
                        self.term_illegal()
                    }
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
            10 => {
                if self.rca == u16::try_from((arg >> 16) & 0xffff).unwrap() {
                    if self.card_status.get_current_state() == CurrentState::StandBy {
                        Response::R2(ResponseType2 { cid_csd: CID_ESD.clone() })
                    } else {
                        self.term_illegal()
                    }
                } else {
                    warn!("RCA does not match, ignoring request.");
                    Response::RNone
                }
            }
            12 => {
                match self.card_status.get_current_state() {
                    CurrentState::SendingData | CurrentState::ReceivingData => {
                        // TODO: ReceivingData technically needs to wait until the write buffer has been flushed.
                        // We don't implement asychronous IO operations yet so switching directly to Transfer is good enough for now.
                        trace!("Continuous data IO end");
                        self.recv_action = RecvAction::None;
                        self.send_action = SendAction::None;
                        self.card_status.set_current_state(CurrentState::Transfer);
                        let status = self.card_status.after_read();
                        Response::R1(ResponseType1 { cmd, status, busy: false })
                    }
                    _ => self.term_illegal(),
                }
            }
            16 => {
                if self.card_status.get_current_state() == CurrentState::Transfer {
                    self.card_status.after_read();
                    if arg == 0 || arg > 512 {
                        warn!("New IO size of {arg} bytes is out of range 1..=512.");
                        self.card_status.set_block_len_error(true);
                        Response::R1(ResponseType1 { cmd, status: self.card_status, busy: false })
                    } else {
                        self.io_size = arg;
                        self.card_status.set_block_len_error(false);
                        debug!("IO size (block length) changed to {} bytes.", self.io_size);
                        Response::R1(ResponseType1 { cmd, status: self.card_status, busy: false })
                    }
                } else {
                    self.term_illegal()
                }
            }
            18 => {
                if self.card_status.get_current_state() == CurrentState::Transfer {
                    self.recv_action = RecvAction::FTLRead { sector_index: arg.into() };
                    self.card_status.set_current_state(CurrentState::SendingData);
                    let status = self.card_status.after_read();
                    Response::R1(ResponseType1 { cmd, status, busy: false })
                } else {
                    self.term_illegal()
                }
            }
            55 => {
                self.card_status.after_read();
                self.card_status.set_app_command(true);
                Response::R1(ResponseType1 { cmd, status: self.card_status, busy: false })
            }
            _ => {
                warn!("Unhandled SD card command {cmd}");
                self.term_illegal()
            }
        }
    }

    /// Send data to the emulated SD card through the DAT channel.
    pub fn send_data(&mut self, data: &[u8]) {
        todo!()
    }

    /// Receive data from the emulated SD card through the DAT channel.
    pub fn recv_data(&mut self, data: &mut [u8]) {
        match self.recv_action {
            RecvAction::None => {
                warn!("Data requested by SIC but no recv_action defined here. \
                       This is likely a bug of either the emulator or the guest program.");
            },
            RecvAction::FTLRead { sector_index } => {
                if data.len() % 512 != 0 {
                    warn!("Buffer size is not multiple of sectors");
                }

                let image_file = self.image_file.as_mut().unwrap();
                image_file.seek(SeekFrom::Start(512 * sector_index)).unwrap_or_else(|err| {
                    error!("Seeking to sector {sector_index} failed: {err:?}");
                    0u64
                });

                image_file.read_exact(data).unwrap_or_else(|err| {
                    error!("Reading {} bytes from sector {} failed: {:?}", data.len(), sector_index, err);
                });

                trace!("Read {} bytes from sector {}", data.len(), sector_index);

                let new_sector_index = sector_index + u64::try_from(data.len()).unwrap() / 512;
                self.recv_action = RecvAction::FTLRead { sector_index: new_sector_index };
            },
            RecvAction::FunctionStatus{arg} => {
                if data.len() < 64 {
                    error!("Buffer is too small for Function Status");
                    return;
                }

                // 200mA
                data[0..2].clone_from_slice(&200u16.to_be_bytes());

                let commit = arg & 0x80000000 != 0;
                let mut ret_status = 0u32;
                for group in 0u8..6u8 {
                    let from_offset = usize::from(group + 1) * 2;
                    let to_offset = from_offset + 2;
                    data[from_offset..to_offset].clone_from_slice(&CARD_FUNC[usize::from(group)].to_be_bytes());

                    let grp_shifts = 4 * group;
                    let new_func = (arg >> grp_shifts) & 0xf;
                    if new_func == 0xf {
                        // Keep
                        ret_status |= self.selected_functions & (0xf << grp_shifts);
                    } else if u16::try_from(1 << new_func).unwrap() & CARD_FUNC[usize::from(5 - group)] != 0 {
                        // Possible to select
                        ret_status |= new_func << grp_shifts;
                    } else {
                        // Not possible to select
                        error!("Unable to select unsupported function group {} function {}.", group + 1, new_func);
                        ret_status |= (0xf << grp_shifts) | 0x80000000;
                    }
                }

                // New functions
                data[14..17].clone_from_slice(&ret_status.to_be_bytes()[1..4]);

                // Function version 2 - has busy flags
                data[17] = 0x01;

                // Busy flags
                data[18..30].fill(0);

                // Reserved
                data[30..].fill(0);

                if commit && ret_status & 0x80000000 == 0 {
                    self.selected_functions = ret_status & 0xffffff;
                }

                debug!("Function Status: {:02x?}", data);

                self.card_status.set_current_state(CurrentState::Transfer);
                self.recv_action = RecvAction::None;
            },
            RecvAction::SCRRead => {
                if data.len() < 8 {
                    error!("Buffer is too small for SCR");
                    return;
                }
                debug!("SCR={SCR:02x?}");
                data[..8].clone_from_slice(&SCR);
                self.card_status.set_current_state(CurrentState::Transfer);
                self.recv_action = RecvAction::None;
            },
        }
    }

    /// Set the `ILLEGAL_COMMAND` status bit and respond with a no response. Should always use with a return.
    #[inline(always)]
    fn term_illegal(&mut self) -> Response {
        self.card_status.set_illegal_command(true);
        Response::RNone
    }
}
