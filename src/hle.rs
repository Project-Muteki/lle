use std::fmt::Write;

use bitflags::bitflags;
use log::{error, info, warn};
use regex::Regex;
use unicorn_engine::{RegisterARM, uc_error};

use crate::{RuntimeError, device::{QuitDetail, UnicornContext, request_stop}};

const FORMAT_REGEX: &str = concat!(
    r"%(?:(?<escape>%)|",
        r"(?<flags>[-+ #0]+)?",
        r"(?<width>[0-9]+)?",
        r"(?:\.(?<precision>[0-9]+))?",
        r"(?<length>hh|h|l|ll|j|z|t|L)?",
        r"(?<specifier>[csdioxXufFeEaAgGnp])",
    r")"
);

bitflags! {
    #[derive(Debug)]
    pub struct FormatFlags: u8 {
        const LeftJustified = 1 << 0;
        const AlwaysSign = 1 << 1;
        const PadZero = 1 << 2;
        const PadSpace = 1 << 3;
        const AltMode = 1 << 4;
        const DynamicWidth = 1 << 5;
        const DynamicPrecision = 1 << 6;
        const Capital = 1 << 7;
    }
}

#[derive(Default, Debug, PartialEq)]
pub enum LengthModifier {
    /// Quarter-width data (the `hh` modifier).
    Quarter,
    /// Half-width data (the `h` modifier).
    Half,
    /// Full-width data (no modifier).
    #[default]
    Full,
    /// Double-width data (the `l` modifier).
    Double,
    /// Quadruple-width data (the `L` modifier for float types and `ll` for integer types).
    Quadruple,
    /// `(u)intmax_t` (the `j` modifier).
    IntMax,
    /// `(s)size_t` (the `z` modifier).
    Size,
    /// `(u)ptrdiff_t` (the `t` modifier).
    PointerOffset,
}

#[derive(Debug)]
pub enum IntegerType {
    SignedDecimal,
    UnsignedDecimal,
    Octal,
    Hexadecimal,
}

#[derive(Debug)]
pub enum FloatType {
    Normal,
    DecimalExponent,
    HexadecimalExponent,
    AutoExponent,
}

#[derive(Debug)]
pub struct NumericalFormat {
    padding: usize,
    precision: usize,
    length: LengthModifier,
    flags: FormatFlags,
}

#[derive(Debug)]
pub enum ConversionSegment {
    Literal{start: usize, end: usize},
    Escape,
    Character{flags: FormatFlags, padding: Option<usize>},
    String{flags: FormatFlags, padding: Option<usize>, limit: Option<usize>},
    Integer{format: NumericalFormat, type_: IntegerType},
    Float{format: NumericalFormat, type_: FloatType},
}

#[derive(Debug)]
pub struct FormatString {
    raw: String,
    parsed: Vec<ConversionSegment>,
}

impl From<String> for FormatString {
    fn from(value: String) -> Self {
        let mut obj = Self { raw: value, parsed: vec![] };
        let regex = Regex::new(FORMAT_REGEX).unwrap();
        let mut literal_offset = 0usize;
        for c in regex.captures_iter(&obj.raw) {
            let m = c.get(0).unwrap();
            if m.start() != literal_offset {
                obj.parsed.push(ConversionSegment::Literal { start: literal_offset, end: m.start() });
                literal_offset = m.start();
            }

            literal_offset += m.len();

            if c.name("escape").is_some() {
                obj.parsed.push(ConversionSegment::Escape);
                continue;
            }

            let ff_flags = if let Some(m_flags) = c.name("flags") {
                let flags_str = m_flags.as_str();
                flags_str.chars().fold(FormatFlags::empty(), |acc, flag| {
                    acc | match flag {
                        '-' => FormatFlags::LeftJustified,
                        '+' => FormatFlags::AlwaysSign,
                        ' ' => FormatFlags::PadSpace,
                        '#' => FormatFlags::AltMode,
                        '0' => FormatFlags::PadZero,
                        _ => FormatFlags::empty(),
                    }
                })
            } else { FormatFlags::empty() };

            let width = if let Some(m_padding) = c.name("width") {
                m_padding.as_str().parse::<usize>().ok()
            } else {
                None
            };

            let precision = if let Some(m_padding) = c.name("precision") {
                m_padding.as_str().parse::<usize>().ok()
            } else {
                None
            };

            let length = if let Some(m_length) = c.name("length") {
                match m_length.as_str() {
                    "hh" => LengthModifier::Quarter,
                    "h" => LengthModifier::Half,
                    "l" => LengthModifier::Double,
                    "ll" | "L" => LengthModifier::Quadruple,
                    "j" => LengthModifier::IntMax,
                    "z" => LengthModifier::Size,
                    "t" => LengthModifier::PointerOffset,
                    _ => LengthModifier::Full,
                }
            } else {
                LengthModifier::Full
            };

            let specifier = c.name("specifier").unwrap().as_str();

            match specifier {
                "c" => obj.parsed.push(ConversionSegment::Character { flags: ff_flags, padding: width }),
                "s" => obj.parsed.push(ConversionSegment::String { flags: ff_flags, padding: width, limit: precision }),
                "d" | "i" | "o" | "x" | "X" | "u" => obj.parsed.push(ConversionSegment::Integer {
                    format: NumericalFormat {
                        padding: width.unwrap_or(0usize),
                        precision: precision.unwrap_or(1usize),
                        length,
                        flags: ff_flags | if specifier.to_uppercase() == specifier {
                            FormatFlags::Capital
                        } else {
                            FormatFlags::empty()
                        },
                    },
                    type_: match specifier {
                        "d" | "i" => IntegerType::SignedDecimal,
                        "o" => IntegerType::Octal,
                        "x" | "X" => IntegerType::Hexadecimal,
                        "u" => IntegerType::UnsignedDecimal,
                        _ => panic!(),
                    },
                }),
                "f" | "F" | "e" | "E" | "a" | "A" | "g" | "G" => obj.parsed.push(ConversionSegment::Float {
                    format: NumericalFormat {
                        padding: width.unwrap_or(0usize),
                        precision: precision.unwrap_or(1usize),
                        length,
                        flags: ff_flags | if specifier.to_uppercase() == specifier {
                            FormatFlags::Capital
                        } else {
                            FormatFlags::empty()
                        },
                    },
                    type_: match specifier {
                        "f" | "F" => FloatType::Normal,
                        "e" | "E" => FloatType::DecimalExponent,
                        "a" | "A" => FloatType::HexadecimalExponent,
                        "g" | "G" => FloatType::AutoExponent,
                        _ => panic!(),
                    },
                }),
                _ => {
                    warn!("Unhandled specifier {specifier}");
                }
            }
        }
        if literal_offset < obj.raw.len() {
            obj.parsed.push(ConversionSegment::Literal { start: literal_offset, end: obj.raw.len() });
        }
        obj 
    }
}

pub fn get_arg_at(uc: &UnicornContext, pos: u64) -> Result<u32, uc_error> {
    match pos {
        0 => Ok(u32::try_from(uc.reg_read(RegisterARM::R0)? & 0xffffffff).unwrap()),
        1 => Ok(u32::try_from(uc.reg_read(RegisterARM::R1)? & 0xffffffff).unwrap()),
        2 => Ok(u32::try_from(uc.reg_read(RegisterARM::R2)? & 0xffffffff).unwrap()),
        3 => Ok(u32::try_from(uc.reg_read(RegisterARM::R3)? & 0xffffffff).unwrap()),
        _ => {
            let stack_offset = 4 * (pos - 4) + uc.reg_read(RegisterARM::SP)?;
            let mut bytes = [0u8; 4];
            uc.mem_read(stack_offset, &mut bytes)?;
            Ok(u32::from_le_bytes(bytes))
        }
    }
}

#[test]
fn test() {
    let s = String::from("Hello %01.2d%02X world!");
    let fmt = FormatString::from(s);
    println!("{fmt:?}");
}

fn read_cstr(uc: &UnicornContext, address: u64) -> Result<String, RuntimeError> {
    let mut tmp = [0u8; 256];
    let mut result: Vec<u8> = vec![];
    // HACK: Manually fix pointers after TLB. We need a proper way of looking up pointers when needed.
    let mut current_address = if address < 0x2000 {
        address + 0xff000000
    } else {
        address
    };
    loop {
        uc.mem_read(current_address, &mut tmp)?;
        let copy_size = tmp.iter().position(|e| *e == 0).unwrap_or(tmp.len());
        result.extend_from_slice(&tmp[..copy_size]);
        if copy_size < tmp.len() {
            break;
        }
        current_address += u64::try_from(tmp.len()).unwrap();
    }

    let result_str = String::from_utf8(result)?;
    Ok(result_str)
}

// TODO actually implement the correct padding behavior and finish it
fn printf(uc: &mut UnicornContext) -> Result<(), RuntimeError> {
    let fmt_offset = uc.reg_read(RegisterARM::R0)?;

    let mut out = String::new();
    let fmt = read_cstr(uc, fmt_offset)?;
    let fmt_obj = FormatString::from(fmt);
    let mut offset = 1u64;
    for conv in fmt_obj.parsed.iter() {
        match conv {
            ConversionSegment::Literal { start, end } => {
                write!(&mut out, "{}", &fmt_obj.raw[*start..*end])?;
            },
            ConversionSegment::Escape => {
                write!(&mut out, "%")?;
            },
            ConversionSegment::Character { flags, padding } => {
                let arg = get_arg_at(uc, offset)?;
                offset += 1;
                write!(&mut out, "{}", char::from_u32(arg).unwrap())?;
            },
            ConversionSegment::String { flags, padding, limit } => {
                let arg = get_arg_at(uc, offset)?;
                offset += 1;
                let s = read_cstr(uc, arg.into())?;
                write!(&mut out, "{}", s)?;
            },
            ConversionSegment::Integer { format, type_ } => {
                match type_ {
                    IntegerType::SignedDecimal => {
                        match format.length {
                            LengthModifier::Quarter => {
                                write!(&mut out, "{}", (get_arg_at(uc, offset)? & 0xff) as i8)?;
                                offset += 1;
                            },
                            LengthModifier::Half => {
                                write!(&mut out, "{}", (get_arg_at(uc, offset)? & 0xffff) as i16)?;
                                offset += 1;
                            },
                            LengthModifier::Full | LengthModifier::Double => {
                                write!(&mut out, "{}", (get_arg_at(uc, offset)? & 0xffffffff) as i32)?;
                                offset += 1;
                            },
                            LengthModifier::Quadruple => {
                                let a: u64 = get_arg_at(uc, offset)?.into();
                                let b: u64 = get_arg_at(uc, offset + 4)?.into();
                                write!(&mut out, "{}", (b << 32 | a) as i64)?;
                                offset += 2;
                            },
                            LengthModifier::IntMax => todo!(),
                            LengthModifier::Size => todo!(),
                            LengthModifier::PointerOffset => todo!(),
                        }
                    },
                    IntegerType::UnsignedDecimal => {
                        match format.length {
                            LengthModifier::Quarter => {
                                write!(&mut out, "{}", get_arg_at(uc, offset)? & 0xff)?;
                                offset += 1;
                            },
                            LengthModifier::Half => {
                                write!(&mut out, "{}", get_arg_at(uc, offset)? & 0xffff)?;
                                offset += 1;
                            },
                            LengthModifier::Full | LengthModifier::Double => {
                                write!(&mut out, "{}", get_arg_at(uc, offset)? & 0xffffffff)?;
                                offset += 1;
                            },
                            LengthModifier::Quadruple => {
                                let a: u64 = get_arg_at(uc, offset)?.into();
                                let b: u64 = get_arg_at(uc, offset + 4)?.into();
                                write!(&mut out, "{}", b << 32 | a)?;
                                offset += 2;
                            },
                            LengthModifier::IntMax => todo!(),
                            LengthModifier::Size => todo!(),
                            LengthModifier::PointerOffset => todo!(),
                        }
                    },
                    IntegerType::Octal => todo!(),
                    IntegerType::Hexadecimal => {
                        match format.length {
                            LengthModifier::Quarter => {
                                write!(&mut out, "{:x}", get_arg_at(uc, offset)? & 0xff)?;
                                offset += 1;
                            },
                            LengthModifier::Half => {
                                write!(&mut out, "{:x}", get_arg_at(uc, offset)? & 0xffff)?;
                                offset += 1;
                            },
                            LengthModifier::Full | LengthModifier::Double => {
                                write!(&mut out, "{:x}", get_arg_at(uc, offset)? & 0xffffffff)?;
                                offset += 1;
                            },
                            LengthModifier::Quadruple => {
                                let a: u64 = get_arg_at(uc, offset)?.into();
                                let b: u64 = get_arg_at(uc, offset + 4)?.into();
                                write!(&mut out, "{:x}", b << 32 | a)?;
                                offset += 2;
                            },
                            LengthModifier::IntMax => todo!(),
                            LengthModifier::Size => todo!(),
                            LengthModifier::PointerOffset => todo!(),
                        }
                    },
                }
            },
            ConversionSegment::Float { format, type_ } => todo!(),
        }
    }
    info!("{}", &out.trim());
    Ok(())
}

pub fn printf_callback(uc: &mut UnicornContext, _addr: u64, _size: u32) {
    printf(uc).unwrap_or_else(|err| {
        let lr = uc.reg_read(RegisterARM::LR).unwrap();
        error!("Failed to execute printf at 0x{lr:08x}: {err:?}");
        request_stop(uc, crate::device::StopReason::Quit(QuitDetail::HLECallbackFailure));
    })
}
