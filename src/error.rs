use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsoError {
    BufferTooSmall,
    UnexpectedEof,
    TrailingBytes { remaining: usize },
    InvalidMti,
    InvalidFieldId(u8),
    UnsupportedField(u8),
    MissingField(u8),
    InvalidLength { field: u8, expected: usize, actual: usize },
    LengthExceedsMax { field: u8, max: usize, actual: usize },
    InvalidCharacter { field: u8, byte: u8 },
    InvalidLengthPrefix { field: u8 },
    InvalidComposite { field: u8 },
}

impl fmt::Display for IsoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooSmall => write!(f, "output buffer too small"),
            Self::UnexpectedEof => write!(f, "unexpected end of input buffer"),
            Self::TrailingBytes { remaining } => {
                write!(f, "unexpected trailing bytes after message: {}", remaining)
            }
            Self::InvalidMti => write!(f, "invalid MTI (must be 4 ASCII digits)"),
            Self::InvalidFieldId(id) => write!(f, "invalid field ID: {}", id),
            Self::UnsupportedField(id) => write!(f, "unsupported field ID: {}", id),
            Self::MissingField(id) => write!(f, "missing required field DE{}", id),
            Self::InvalidLength { field, expected, actual } => {
                write!(f, "DE{}: expected length {}, got {}", field, expected, actual)
            }
            Self::LengthExceedsMax { field, max, actual } => {
                write!(f, "DE{}: length {} exceeds max {}", field, actual, max)
            }
            Self::InvalidCharacter { field, byte } => {
                write!(f, "DE{}: invalid byte 0x{:02X}", field, byte)
            }
            Self::InvalidLengthPrefix { field } => {
                write!(f, "DE{}: invalid length prefix", field)
            }
            Self::InvalidComposite { field } => {
                write!(f, "DE{}: invalid composite subfield format", field)
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for IsoError {}