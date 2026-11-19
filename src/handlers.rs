use crate::builder::BufWriter;
use crate::error::IsoError;
use crate::parser::BufReader;

pub(crate) type BuildFn =
    fn(spec: &FieldSpec, val: &[u8], w: &mut BufWriter<'_>) -> Result<(), IsoError>;
pub(crate) type ParseFn =
    for<'a> fn(spec: &FieldSpec, r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharSet {
    Numeric,      // '0'..='9'
    NumSpec,      // '0'..='9' + numeric special symbols
    Track2,       // '0'..='9', '=', 'D', 'd'
    AlphaNum,     // ASCII alphanumeric + space
    AlphaOrNum,   // Homogeneous: all ASCII alphabetic OR all ASCII digits (a or n)
    AlphaNumSpec, // Printable ASCII (0x20..=0x7E)
    Binary,       // Raw bytes
}

impl CharSet {
    #[inline]
    pub(crate) const fn is_valid_byte(self, b: u8) -> bool {
        match self {
            Self::Numeric => b.is_ascii_digit(),
            Self::NumSpec => b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b' '),
            Self::Track2 => b.is_ascii_digit() || matches!(b, b'=' | b'D' | b'd'),
            Self::AlphaNum => b.is_ascii_alphanumeric() || b == b' ',
            Self::AlphaOrNum => b.is_ascii_alphanumeric(),
            Self::AlphaNumSpec => b >= 0x20 && b <= 0x7E,
            Self::Binary => true,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FieldSpec {
    pub(crate) id: u8,
    /// For fixed fields: exact wire length (digit count for x+n; byte count for b).
    /// For variable fields: maximum allowed payload length.
    pub(crate) max_len: u16,
    pub(crate) charset: CharSet,
    pub(crate) build: BuildFn,
    pub(crate) parse: ParseFn,
}

#[inline]
fn validate_bytes(field: u8, charset: CharSet, data: &[u8]) -> Result<(), IsoError> {
    match charset {
        CharSet::Binary => Ok(()),
        CharSet::AlphaOrNum => {
            if data.iter().all(|b| b.is_ascii_digit())
                || data.iter().all(|b| b.is_ascii_alphabetic())
            {
                return Ok(());
            }
            let first_is_digit = data.first().is_some_and(|b| b.is_ascii_digit());
            let bad = data
                .iter()
                .copied()
                .find(|&b| {
                    if first_is_digit {
                        !b.is_ascii_digit()
                    } else {
                        !b.is_ascii_alphabetic()
                    }
                })
                .unwrap_or(b' ');
            Err(IsoError::InvalidCharacter { field, byte: bad })
        }
        _ => {
            for &b in data {
                if !charset.is_valid_byte(b) {
                    return Err(IsoError::InvalidCharacter { field, byte: b });
                }
            }
            Ok(())
        }
    }
}

// ============================================================================
// BUILD & PARSE HANDLERS (7 WIRE HANDLERS + UNUSED GUARD)
// ============================================================================

fn build_unsupported(spec: &FieldSpec, _val: &[u8], _w: &mut BufWriter<'_>) -> Result<(), IsoError> {
    Err(IsoError::UnsupportedField(spec.id))
}

fn parse_unsupported<'a>(spec: &FieldSpec, _r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError> {
    Err(IsoError::UnsupportedField(spec.id))
}

// 1. FixedNum (n X) - right-aligned, zero-padded on the left when building
fn build_fixed_num(spec: &FieldSpec, val: &[u8], w: &mut BufWriter<'_>) -> Result<(), IsoError> {
    let expected = spec.max_len as usize;
    if val.is_empty() {
        return Err(IsoError::InvalidLength { field: spec.id, expected, actual: 0 });
    }
    if val.len() > expected {
        return Err(IsoError::LengthExceedsMax { field: spec.id, max: expected, actual: val.len() });
    }
    validate_bytes(spec.id, spec.charset, val)?;
    let pad = expected - val.len();
    if pad > 0 {
        w.write_repeat(b'0', pad)?;
    }
    w.write_bytes(val)
}

fn parse_fixed_num<'a>(spec: &FieldSpec, r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError> {
    let slice = r.read_exact(spec.max_len as usize)?;
    validate_bytes(spec.id, spec.charset, slice)?;
    Ok(slice)
}

// 2. FixedSignedNum (x+n X -> 'C'/'D' + X digits, zero-padded after the sign byte)
fn build_fixed_signed(spec: &FieldSpec, val: &[u8], w: &mut BufWriter<'_>) -> Result<(), IsoError> {
    let expected_digits = spec.max_len as usize;
    let max_total = 1 + expected_digits;
    if val.is_empty() {
        return Err(IsoError::InvalidLength { field: spec.id, expected: max_total, actual: 0 });
    }
    if val.len() > max_total {
        return Err(IsoError::LengthExceedsMax { field: spec.id, max: max_total, actual: val.len() });
    }
    if !matches!(val[0], b'C' | b'D') {
        return Err(IsoError::InvalidCharacter { field: spec.id, byte: val[0] });
    }
    let digits = &val[1..];
    if digits.is_empty() {
        return Err(IsoError::InvalidLength { field: spec.id, expected: max_total, actual: 1 });
    }
    validate_bytes(spec.id, CharSet::Numeric, digits)?;
    w.write_bytes(&val[0..1])?;
    let pad = expected_digits - digits.len();
    if pad > 0 {
        w.write_repeat(b'0', pad)?;
    }
    w.write_bytes(digits)
}

fn parse_fixed_signed<'a>(spec: &FieldSpec, r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError> {
    let slice = r.read_exact(1 + spec.max_len as usize)?;
    if !matches!(slice[0], b'C' | b'D') {
        return Err(IsoError::InvalidCharacter { field: spec.id, byte: slice[0] });
    }
    validate_bytes(spec.id, CharSet::Numeric, &slice[1..])?;
    Ok(slice)
}

// 3. FixedAlpha (a / an / ans / a|n X) - left-aligned, space-padded on the right (exact for a|n)
fn build_fixed_alpha(spec: &FieldSpec, val: &[u8], w: &mut BufWriter<'_>) -> Result<(), IsoError> {
    let expected = spec.max_len as usize;
    if matches!(spec.charset, CharSet::AlphaOrNum) && val.len() != expected {
        return Err(IsoError::InvalidLength { field: spec.id, expected, actual: val.len() });
    }
    if val.len() > expected {
        return Err(IsoError::LengthExceedsMax { field: spec.id, max: expected, actual: val.len() });
    }
    validate_bytes(spec.id, spec.charset, val)?;
    w.write_bytes(val)?;
    let pad = expected - val.len();
    if pad > 0 {
        w.write_repeat(b' ', pad)?;
    }
    Ok(())
}

fn parse_fixed_alpha<'a>(spec: &FieldSpec, r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError> {
    let slice = r.read_exact(spec.max_len as usize)?;
    validate_bytes(spec.id, spec.charset, slice)?;
    Ok(slice)
}

// 4. FixedBin (b X bits -> X/8 bytes) - strict exact length, no silent padding
fn build_fixed_bin(spec: &FieldSpec, val: &[u8], w: &mut BufWriter<'_>) -> Result<(), IsoError> {
    let expected = spec.max_len as usize;
    if val.len() != expected {
        return Err(IsoError::InvalidLength { field: spec.id, expected, actual: val.len() });
    }
    w.write_bytes(val)
}

fn parse_fixed_bin<'a>(spec: &FieldSpec, r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError> {
    r.read_exact(spec.max_len as usize)
}

// 5. LlVarNum (n.. / ns.. / z..) - kept separate from LlVarAlpha for future BCD/packed dialects
fn build_ll_num(spec: &FieldSpec, val: &[u8], w: &mut BufWriter<'_>) -> Result<(), IsoError> {
    let max = spec.max_len as usize;
    if val.len() > max || val.len() > 99 {
        return Err(IsoError::LengthExceedsMax { field: spec.id, max, actual: val.len() });
    }
    validate_bytes(spec.id, spec.charset, val)?;
    let ll = [((val.len() / 10) as u8) + b'0', ((val.len() % 10) as u8) + b'0'];
    w.write_bytes(&ll)?;
    w.write_bytes(val)
}

fn parse_ll_num<'a>(spec: &FieldSpec, r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError> {
    let len = read_ascii_len(spec.id, r, 2)?;
    if len > spec.max_len as usize {
        return Err(IsoError::LengthExceedsMax {
            field: spec.id,
            max: spec.max_len as usize,
            actual: len,
        });
    }
    let slice = r.read_exact(len)?;
    validate_bytes(spec.id, spec.charset, slice)?;
    Ok(slice)
}

// 6. LlVarAlpha (an.. / ans..) - kept separate from LlVarNum for future EBCDIC/BCD dialects
fn build_ll_alpha(spec: &FieldSpec, val: &[u8], w: &mut BufWriter<'_>) -> Result<(), IsoError> {
    let max = spec.max_len as usize;
    if val.len() > max || val.len() > 99 {
        return Err(IsoError::LengthExceedsMax { field: spec.id, max, actual: val.len() });
    }
    validate_bytes(spec.id, spec.charset, val)?;
    let ll = [((val.len() / 10) as u8) + b'0', ((val.len() % 10) as u8) + b'0'];
    w.write_bytes(&ll)?;
    w.write_bytes(val)
}

fn parse_ll_alpha<'a>(spec: &FieldSpec, r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError> {
    let len = read_ascii_len(spec.id, r, 2)?;
    if len > spec.max_len as usize {
        return Err(IsoError::LengthExceedsMax {
            field: spec.id,
            max: spec.max_len as usize,
            actual: len,
        });
    }
    let slice = r.read_exact(len)?;
    validate_bytes(spec.id, spec.charset, slice)?;
    Ok(slice)
}

// 7. LllVar (n... / an... / ans... / composite fields)
fn build_lll_var(spec: &FieldSpec, val: &[u8], w: &mut BufWriter<'_>) -> Result<(), IsoError> {
    let max = spec.max_len as usize;
    if val.len() > max || val.len() > 999 {
        return Err(IsoError::LengthExceedsMax { field: spec.id, max, actual: val.len() });
    }
    validate_bytes(spec.id, spec.charset, val)?;
    let lll = [
        ((val.len() / 100) as u8) + b'0',
        (((val.len() / 10) % 10) as u8) + b'0',
        ((val.len() % 10) as u8) + b'0',
    ];
    w.write_bytes(&lll)?;
    w.write_bytes(val)
}

fn parse_lll_var<'a>(spec: &FieldSpec, r: &mut BufReader<'a>) -> Result<&'a [u8], IsoError> {
    let len = read_ascii_len(spec.id, r, 3)?;
    if len > spec.max_len as usize {
        return Err(IsoError::LengthExceedsMax {
            field: spec.id,
            max: spec.max_len as usize,
            actual: len,
        });
    }
    let slice = r.read_exact(len)?;
    validate_bytes(spec.id, spec.charset, slice)?;
    Ok(slice)
}

#[inline]
fn read_ascii_len(field: u8, r: &mut BufReader<'_>, digits: usize) -> Result<usize, IsoError> {
    let bytes = r.read_exact(digits)?;
    let mut len: usize = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return Err(IsoError::InvalidLengthPrefix { field });
        }
        len = len * 10 + (b - b'0') as usize;
    }
    Ok(len)
}

// ============================================================================
// CONST CONSTRUCTORS & STATIC FIELD TABLE FOR DE 0..=128 (ISO 8583:1987)
// ============================================================================

const fn f_unused(id: u8) -> FieldSpec {
    FieldSpec { id, max_len: 0, charset: CharSet::Binary, build: build_unsupported, parse: parse_unsupported }
}
const fn f_num(id: u8, len: u16) -> FieldSpec {
    FieldSpec { id, max_len: len, charset: CharSet::Numeric, build: build_fixed_num, parse: parse_fixed_num }
}
const fn f_sgn(id: u8, len: u16) -> FieldSpec {
    FieldSpec { id, max_len: len, charset: CharSet::Numeric, build: build_fixed_signed, parse: parse_fixed_signed }
}
const fn f_alp(id: u8, len: u16, charset: CharSet) -> FieldSpec {
    FieldSpec { id, max_len: len, charset, build: build_fixed_alpha, parse: parse_fixed_alpha }
}
const fn f_bin(id: u8, bytes: u16) -> FieldSpec {
    FieldSpec { id, max_len: bytes, charset: CharSet::Binary, build: build_fixed_bin, parse: parse_fixed_bin }
}
const fn ll_num(id: u8, max: u16, charset: CharSet) -> FieldSpec {
    FieldSpec { id, max_len: max, charset, build: build_ll_num, parse: parse_ll_num }
}
const fn ll_alp(id: u8, max: u16, charset: CharSet) -> FieldSpec {
    FieldSpec { id, max_len: max, charset, build: build_ll_alpha, parse: parse_ll_alpha }
}
const fn lll_v(id: u8, max: u16, charset: CharSet) -> FieldSpec {
    FieldSpec { id, max_len: max, charset, build: build_lll_var, parse: parse_lll_var }
}

use CharSet::{
    AlphaNum as AN, AlphaNumSpec as ANS, AlphaOrNum as A_OR_N, NumSpec as NS, Numeric as N,
    Track2 as Z,
};

pub(crate) static FIELD_TABLE: [FieldSpec; 129] = [
    f_unused(0),        // 0: Unused sentinel
    f_unused(1),        // 1: Bitmap (handled structurally by builder/parser)
    ll_num(2, 19, N),   // 2: Primary account number (PAN)
    f_num(3, 6),        // 3: Processing code
    f_num(4, 12),       // 4: Amount, transaction
    f_num(5, 12),       // 5: Amount, settlement
    f_num(6, 12),       // 6: Amount, cardholder billing
    f_num(7, 10),       // 7: Transmission date & time
    f_num(8, 8),        // 8: Amount, cardholder billing fee
    f_num(9, 8),        // 9: Conversion rate, settlement
    f_num(10, 8),       // 10: Conversion rate, cardholder billing
    f_num(11, 6),       // 11: System trace audit number (STAN)
    f_num(12, 6),       // 12: Local transaction time
    f_num(13, 4),       // 13: Local transaction date
    f_num(14, 4),       // 14: Expiration date
    f_num(15, 4),       // 15: Settlement date
    f_num(16, 4),       // 16: Currency conversion date
    f_num(17, 4),       // 17: Capture date
    f_num(18, 4),       // 18: Merchant category code
    f_num(19, 3),       // 19: Acquiring institution country code
    f_num(20, 3),       // 20: PAN extended country code
    f_num(21, 3),       // 21: Forwarding institution country code
    f_num(22, 3),       // 22: POS entry mode
    f_num(23, 3),       // 23: Application PAN sequence number
    f_num(24, 3),       // 24: Network International Identifier (NII)
    f_num(25, 2),       // 25: POS condition code
    f_num(26, 2),       // 26: POS capture code
    f_num(27, 1),       // 27: Authorizing identification response length
    f_sgn(28, 8),       // 28: Amount, transaction fee
    f_sgn(29, 8),       // 29: Amount, settlement fee
    f_sgn(30, 8),       // 30: Amount, transaction processing fee
    f_sgn(31, 8),       // 31: Amount, settlement processing fee
    ll_num(32, 11, N),  // 32: Acquiring institution ID
    ll_num(33, 11, N),  // 33: Forwarding institution ID
    ll_num(34, 28, NS), // 34: Primary account number, extended
    ll_num(35, 37, Z),  // 35: Track 2 data
    lll_v(36, 104, N),  // 36: Track 3 data
    f_alp(37, 12, AN),  // 37: Retrieval reference number
    f_alp(38, 6, AN),   // 38: Authorization identification response
    f_alp(39, 2, AN),   // 39: Response code
    f_alp(40, 3, AN),   // 40: Service restriction code
    f_alp(41, 8, ANS),  // 41: Card acceptor terminal identification
    f_alp(42, 15, ANS), // 42: Card acceptor identification code
    f_alp(43, 40, ANS), // 43: Card acceptor name/location
    ll_alp(44, 25, AN), // 44: Additional response data
    ll_alp(45, 76, ANS),// 45: Track 1 data (ANS supports Track 1 field delimiters)
    lll_v(46, 999, AN), // 46: Additional data (ISO)
    lll_v(47, 999, AN), // 47: Additional data (national)
    lll_v(48, 999, AN), // 48: Additional data (private)
    f_alp(49, 3, A_OR_N), // 49: Currency code, transaction (a or n 3)
    f_alp(50, 3, A_OR_N), // 50: Currency code, settlement (a or n 3)
    f_alp(51, 3, A_OR_N), // 51: Currency code, cardholder billing (a or n 3)
    f_bin(52, 8),       // 52: Personal identification number data (b 64 -> 8 bytes)
    f_num(53, 16),      // 53: Security related control information
    lll_v(54, 120, AN), // 54: Additional amounts
    lll_v(55, 999, ANS),// 55: Reserved ISO (1987: ans...999; EMV binary uses dialect override)
    lll_v(56, 999, ANS),// 56: Reserved ISO
    lll_v(57, 999, ANS),// 57: Reserved national
    lll_v(58, 999, ANS),// 58: Reserved national
    lll_v(59, 999, ANS),// 59: Reserved national
    lll_v(60, 999, ANS),// 60: Reserved national
    lll_v(61, 999, ANS),// 61: Reserved private
    lll_v(62, 999, ANS),// 62: Reserved private
    lll_v(63, 999, ANS),// 63: Reserved private
    f_bin(64, 8),       // 64: Message authentication code (b 64 -> 8 bytes)
    f_unused(65),       // 65: Extended bitmap indicator (1-bit flag, not a byte field in 1987)
    f_num(66, 1),       // 66: Settlement code
    f_num(67, 2),       // 67: Extended payment code
    f_num(68, 3),       // 68: Receiving institution country code
    f_num(69, 3),       // 69: Settlement institution country code
    f_num(70, 3),       // 70: Network management information code
    f_num(71, 4),       // 71: Message number
    f_num(72, 4),       // 72: Last message's number
    f_num(73, 6),       // 73: Action date
    f_num(74, 10),      // 74: Number of credits
    f_num(75, 10),      // 75: Credits, reversal number
    f_num(76, 10),      // 76: Number of debits
    f_num(77, 10),      // 77: Debits, reversal number
    f_num(78, 10),      // 78: Transfer number
    f_num(79, 10),      // 79: Transfer, reversal number
    f_num(80, 10),      // 80: Number of inquiries
    f_num(81, 10),      // 81: Number of authorizations
    f_num(82, 12),      // 82: Credits, processing fee amount
    f_num(83, 12),      // 83: Credits, transaction fee amount
    f_num(84, 12),      // 84: Debits, processing fee amount
    f_num(85, 12),      // 85: Debits, transaction fee amount
    f_num(86, 16),      // 86: Total amount of credits
    f_num(87, 16),      // 87: Credits, reversal amount
    f_num(88, 16),      // 88: Total amount of debits
    f_num(89, 16),      // 89: Debits, reversal amount
    f_num(90, 42),      // 90: Original data elements (fixed-offset composite field)
    f_alp(91, 1, AN),   // 91: File update code
    f_alp(92, 2, AN),   // 92: File security code
    f_alp(93, 5, AN),   // 93: Response indicator
    f_alp(94, 7, AN),   // 94: Service indicator
    f_alp(95, 42, AN),  // 95: Replacement amounts
    f_bin(96, 8),       // 96: Message security code (b 64 -> 8 bytes)
    f_sgn(97, 16),      // 97: Net settlement amount
    f_alp(98, 25, ANS), // 98: Payee
    ll_num(99, 11, N),  // 99: Settlement institution identification code
    ll_num(100, 11, N), // 100: Receiving institution identification code
    ll_alp(101, 17, ANS), // 101: File name
    ll_alp(102, 28, ANS), // 102: Account identification 1
    ll_alp(103, 28, ANS), // 103: Account identification 2
    lll_v(104, 100, ANS), // 104: Transaction description
    lll_v(105, 999, ANS), // 105: Reserved ISO
    lll_v(106, 999, ANS), // 106: Reserved ISO
    lll_v(107, 999, ANS), // 107: Reserved ISO
    lll_v(108, 999, ANS), // 108: Reserved ISO
    lll_v(109, 999, ANS), // 109: Reserved ISO
    lll_v(110, 999, ANS), // 110: Reserved ISO
    lll_v(111, 999, ANS), // 111: Reserved ISO
    lll_v(112, 999, ANS), // 112: Reserved national
    lll_v(113, 999, ANS), // 113: Reserved national
    lll_v(114, 999, ANS), // 114: Reserved national
    lll_v(115, 999, ANS), // 115: Reserved national
    lll_v(116, 999, ANS), // 116: Reserved national
    lll_v(117, 999, ANS), // 117: Reserved national
    lll_v(118, 999, ANS), // 118: Reserved national
    lll_v(119, 999, ANS), // 119: Reserved national
    lll_v(120, 999, ANS), // 120: Reserved private
    lll_v(121, 999, ANS), // 121: Reserved private
    lll_v(122, 999, ANS), // 122: Reserved private
    lll_v(123, 999, ANS), // 123: Reserved private
    lll_v(124, 999, ANS), // 124: Reserved private
    lll_v(125, 999, ANS), // 125: Reserved private
    lll_v(126, 999, ANS), // 126: Reserved private
    lll_v(127, 999, ANS), // 127: Reserved private
    f_bin(128, 8),        // 128: Message authentication code (b 64 -> 8 bytes)
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_table_invariants() {
        for (idx, spec) in FIELD_TABLE.iter().enumerate() {
            assert_eq!(spec.id as usize, idx, "FIELD_TABLE index mismatch at {}", idx);
            if core::ptr::fn_addr_eq(spec.build, build_ll_num as BuildFn)
                || core::ptr::fn_addr_eq(spec.build, build_ll_alpha as BuildFn)
            {
                assert!(spec.max_len <= 99, "DE{} LLVAR max_len exceeds 99", idx);
            } else if core::ptr::fn_addr_eq(spec.build, build_lll_var as BuildFn) {
                assert!(spec.max_len <= 999, "DE{} LLLVAR max_len exceeds 999", idx);
            }
        }
    }
}