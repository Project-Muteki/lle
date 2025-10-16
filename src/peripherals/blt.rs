use bit_field::{B4, B5, bitfield};
use log::{trace, warn};
use crate::{device::{StopReason, UnicornContext, request_stop}, log_unsupported_read, log_unsupported_write, peripherals::aic::{InterruptNumber, post_interrupt}};

pub const BASE: u64 = 0xb100d000;
pub const SIZE: usize = 0x1000;

const REG_SET: u64 = 0x0;
const REG_SFMT: u64 = 0x4;
const REG_DFMT: u64 = 0x8;
const REG_BLTINTCR: u64 = 0xc;
const REG_SWIDTH: u64 = 0x20;
const REG_SHEIGHT: u64 = 0x24;
const REG_DWIDTH: u64 = 0x28;
const REG_DHEIGHT: u64 = 0x2c;
const REG_ELEMENTA: u64 = 0x30;
const REG_ELEMENTB: u64 = 0x34;
const REG_ELEMENTC: u64 = 0x38;
const REG_ELEMENTD: u64 = 0x3c;
const REG_SADDR: u64 = 0x40;
const REG_DADDR: u64 = 0x44;
const REG_SSTRIDE: u64 = 0x48;
const REG_DSTRIDE: u64 = 0x4c;
const REG_OFFSETX: u64 = 0x50;
const REG_OFFSETY: u64 = 0x54;

#[bitfield]
#[derive(Default)]
pub struct BLTFlags {
    trigger: bool,
    palette_le: bool,
    blend_on_fill: bool,
    ignore_src_alpha: bool,
    src_transparency: bool,
    apply_rgba_transform: bool,
    apply_alpha_transform: bool,
    transparent_color: bool,
    fill_clip_to_edge: bool,
    fill_no_smooth: bool,
    fill_none_fill: bool,
    fill: bool,
    reserved_12: B4,
}

#[bitfield]
#[derive(Default)]
pub struct BLTStatus {
    status: bool,
    enabled: bool,
    error: bool,
    reserved: B5
}

#[derive(Default, Debug, Copy, Clone)]
pub enum SourceFormat {
    #[default]
    Unspecified,
    ARGB8888,
    RGB565,
    Pal1,
    Pal2,
    Pal4,
    Pal8,
}

#[derive(Default, Debug, Copy, Clone)]
pub enum DestinationFormat {
    #[default]
    Unspecified,
    ARGB8888,
    RGB565,
    RGB555,
}

impl From<u64> for SourceFormat {
    fn from(value: u64) -> Self {
        let mut tmp = value;
        for fmt in [Self::ARGB8888, Self::RGB565, Self::Pal1, Self::Pal2, Self::Pal4, Self::Pal8] {
            if tmp & 1 == 1 {
                return fmt;
            }
            tmp >>= 1;
        }
        Self::Unspecified
    }
}

impl From<u64> for DestinationFormat {
    fn from(value: u64) -> Self {
        let mut tmp = value;
        for fmt in [Self::ARGB8888, Self::RGB565, Self::RGB555] {
            if tmp & 1 == 1 {
                return fmt;
            }
            tmp >>= 1;
        }
        Self::Unspecified
    }
}

impl Into<u64> for SourceFormat {
    fn into(self) -> u64 {
        1 << ((self as u8) - 1)
    }
}

impl Into<u64> for DestinationFormat {
    fn into(self) -> u64 {
        1 << ((self as u8) - 1)
    }
}

#[derive(Default, Debug)]
pub struct BLTConfig {
    pub flags: BLTFlags,
    pub status: BLTStatus,
    pub src: u32,
    pub dest: u32,
    pub src_format: SourceFormat,
    pub dest_format: DestinationFormat,
    pub src_width: u16,
    pub src_height: u16,
    pub dest_width: u16,
    pub dest_height: u16,
    pub src_pitch: u16,
    pub dest_pitch: u16,

    pub element_a: i32,
    pub element_b: i32,
    pub element_c: i32,
    pub element_d: i32,
    pub translate_x: i32,
    pub translate_y: i32,
}

// #[inline]
// fn fixed1616_to_f32(fixed: i32) -> f32 {
//     (f64::from(fixed) / 65536.0) as f32
// }

pub fn read(uc: &mut UnicornContext, addr: u64, size: usize) -> u64 {
    if size != 4 {
        log_unsupported_read!(addr, size);
        return 0;
    }

    let blt = &uc.get_data().blt;

    match addr {
        REG_SET => blt.flags.get(0, 16),
        REG_SFMT => blt.src_format.into(),
        REG_DFMT => blt.dest_format.into(),
        REG_BLTINTCR => blt.status.get(0, 8),
        REG_SWIDTH => blt.src_width.into(),
        REG_SHEIGHT => blt.src_height.into(),
        REG_DWIDTH => blt.dest_width.into(),
        REG_DHEIGHT => blt.dest_height.into(),
        REG_ELEMENTA => blt.element_a.cast_unsigned().into(),
        REG_ELEMENTB => blt.element_b.cast_unsigned().into(),
        REG_ELEMENTC => blt.element_c.cast_unsigned().into(),
        REG_ELEMENTD => blt.element_d.cast_unsigned().into(),
        REG_SADDR => blt.src.into(),
        REG_DADDR => blt.dest.into(),
        REG_SSTRIDE => blt.src_pitch.into(),
        REG_DSTRIDE => blt.dest_pitch.into(),
        REG_OFFSETX => blt.translate_x.cast_unsigned().into(),
        REG_OFFSETY => blt.translate_y.cast_unsigned().into(),
        _ => {
            log_unsupported_read!(addr, size);
            0
        }
    }
}

pub fn write(uc: &mut UnicornContext, addr: u64, size: usize, value: u64) {
    if size != 4 {
        log_unsupported_write!(addr, size, value);
        return;
    }
    let blt = &mut uc.get_data_mut().blt;
    match addr {
        REG_SET => {
            blt.flags.set(0, 16, value);
            if blt.flags.get_trigger() {
                request_stop(uc, StopReason::Tick);
            }
        },
        REG_SFMT => blt.src_format = SourceFormat::from(value),
        REG_DFMT => blt.dest_format = DestinationFormat::from(value),
        REG_BLTINTCR => {
            if value & 0b1 != 0 {
                blt.status.set_status(false);
            }
            if value & 0b100 != 0 {
                blt.status.set_error(false);
            }
            blt.status.set_enabled(value & 0b10 != 0);
        },
        REG_SWIDTH => blt.src_width = u16::try_from(value & 0xffff).unwrap(),
        REG_SHEIGHT => blt.src_height = u16::try_from(value & 0xffff).unwrap(),
        REG_DWIDTH => blt.dest_width = u16::try_from(value & 0xffff).unwrap(),
        REG_DHEIGHT => blt.dest_height = u16::try_from(value & 0xffff).unwrap(),
        REG_ELEMENTA => blt.element_a = u32::try_from(value & 0xffffffff).unwrap().cast_signed(),
        REG_ELEMENTB => blt.element_b = u32::try_from(value & 0xffffffff).unwrap().cast_signed(),
        REG_ELEMENTC => blt.element_c = u32::try_from(value & 0xffffffff).unwrap().cast_signed(),
        REG_ELEMENTD => blt.element_d = u32::try_from(value & 0xffffffff).unwrap().cast_signed(),
        REG_SADDR => blt.src = u32::try_from(value & 0xffffffff).unwrap(),
        REG_DADDR => blt.dest = u32::try_from(value & 0xffffffff).unwrap(),
        REG_SSTRIDE => blt.src_pitch = u16::try_from(value & 0xffff).unwrap(),
        REG_DSTRIDE => blt.dest_pitch = u16::try_from(value & 0xffff).unwrap(),
        REG_OFFSETX => blt.translate_x = u32::try_from(value & 0xffffffff).unwrap().cast_signed(),
        REG_OFFSETY => blt.translate_y = u32::try_from(value & 0xffffffff).unwrap().cast_signed(),
        _ => {
            log_unsupported_write!(addr, size, value);
        }
    };
}

pub fn tick(uc: &mut UnicornContext) {
    let blt = &uc.get_data().blt;
    if !blt.flags.get_trigger() {
        return;
    }

    trace!("BLIT action {blt:?}");

    if blt.flags.get_fill() {
        warn!("Fill mode not implemented yet.");
        let blt = &mut uc.get_data_mut().blt;
        blt.flags.set_fill(false);
        blt.flags.set_trigger(false);
        blt.status.set_status(true);
        if blt.status.get_enabled() {
            post_interrupt(uc, InterruptNumber::BLT, true, false);
        }
        return;
    }

    let is_identity = 
        blt.element_a == 0x10000 &&
        blt.element_b == 0 &&
        blt.element_c == 0 &&
        blt.element_d == 0x10000;

    // let proj = Projection::from_matrix([
    //     fixed1616_to_f32(blt.element_a), fixed1616_to_f32(blt.element_c), fixed1616_to_f32(blt.translate_x),
    //     fixed1616_to_f32(blt.element_b), fixed1616_to_f32(blt.element_d), fixed1616_to_f32(blt.translate_y),
    //     0.0, 0.0, 1.0,
    // ]);

    // if proj.is_none() {
    //     error!("Cannot build projection matrix from {blt:?}");
    //     let blt = &mut uc.get_data_mut().blt;
    //     blt.status.set_error(true);
    //     blt.status.set_status(true);
    //     blt.flags.set_trigger(false);
    //     if blt.status.get_enabled() {
    //         post_interrupt(uc, InterruptNumber::BLT, true, false);
    //     }
    //     return;
    // }

    //let proj = proj.unwrap();
    if is_identity &&
        blt.src_width == blt.dest_width &&
        blt.src_height == blt.dest_height
    {
        if matches!(blt.src_format, SourceFormat::ARGB8888) &&
            matches!(blt.dest_format, DestinationFormat::RGB565)
        {
            let buf = uc.mem_read_as_vec(blt.src.into(), 320 * 240 * 4).unwrap();
            let mut buf2: Vec<u8> = vec![];
            for pixel in buf.chunks_exact(4) {
                buf2.push((pixel[0] >> 3) | ((pixel[1] & 0b111) << 5));
                buf2.push((pixel[2] & 0xf8) | (pixel[1] >> 5));
            }
            uc.mem_write(blt.dest.into(), &buf2).unwrap();
        } else if matches!(blt.src_format, SourceFormat::RGB565) &&
            matches!(blt.dest_format, DestinationFormat::RGB565)
        {
            let buf = uc.mem_read_as_vec(blt.src.into(), 320 * 240 * 2).unwrap();
            uc.mem_write(blt.dest.into(), &buf).unwrap();
        }
    } else if is_identity {
        if matches!(blt.src_format, SourceFormat::RGB565) &&
            matches!(blt.dest_format, DestinationFormat::RGB565)
        {
            let copy_width = usize::from(blt.src_width.min(blt.dest_width));
            let copy_height = usize::from(blt.src_height.min(blt.dest_height));
            let copy_offset = u64::from((blt.translate_x >> 16).cast_unsigned() * 2 + (blt.translate_y >> 16).cast_unsigned() * u32::from(blt.src_pitch));

            let srcbuf = uc.mem_read_as_vec(u64::from(blt.src) + copy_offset, usize::from(blt.src_pitch) * copy_height).unwrap();
            let mut destbuf = uc.mem_read_as_vec(blt.dest.into(), usize::from(blt.dest_pitch) * copy_height).unwrap();

            for (i, pixel) in srcbuf.chunks_exact(2).enumerate() {
                let line = (i * 2) / usize::from(blt.src_pitch);
                let pxoffset = (i * 2) % usize::from(blt.src_pitch);

                if pxoffset >= copy_width * 2 {
                    continue;
                }

                let copy_offset = line * usize::from(blt.dest_pitch) + pxoffset;
                if copy_offset >= destbuf.len() || copy_offset + 1 >= destbuf.len() {
                    continue;
                }

                destbuf[copy_offset] = pixel[0];
                destbuf[copy_offset + 1] = pixel[1];
            }

            uc.mem_write(blt.dest.into(), &destbuf).unwrap();
        }
    } else {
        todo!();
    }

    let blt = &mut uc.get_data_mut().blt;
    blt.status.set_status(true);
    blt.flags.set_trigger(false);
    if blt.status.get_enabled() {
        post_interrupt(uc, InterruptNumber::BLT, true, false);
    }
}
