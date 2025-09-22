
use std::fs;

use bit_field::B1;
use bit_field::B2;
use bit_field::bitfield;
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

// TODO Find a CID that looks convincing.
const CID_ESD: [u8; 16] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

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
        let reg0 = (0b00111111 << 24) | (u32::from(self.is_sdhc) << 22) | (self.ocr >> 8);
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

#[derive(Default)]
pub struct SD {
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
        // TODO detect SD type (SDSC or SDHC)
        Ok(())
    }

    pub fn unmount(&mut self) {
        self.image_file = None;
    }

    /// Make a request on the CMD channel.
    pub fn make_request(&mut self, cmd: u8, arg: u32) -> Response {
        if self.card_status.get_app_command() == 1 {
            // ACMD
            // TODO
            return match cmd {
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
            55 => {
                self.card_status.set_app_command(1);
                let status = self.card_status.after_read();
                Response::R1(ResponseType1 { cmd, status, busy: false })
            }
            _ => {
                warn!("Unhandled SD card command 0x{cmd:02x}");
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

    /// Set the `ILLEGAL_COMMAND` status bit and respond with a no response.
    #[inline(always)]
    fn term_illegal(&mut self) -> Response {
        self.card_status.set_illegal_command(1);
        Response::RNone
    }
}
