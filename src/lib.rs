#![cfg_attr(not(feature = "std"), no_std)]

mod builder;
pub mod composite;
pub mod de;
mod error;
mod handlers;
mod parser;

pub use builder::{build, FieldIter, IsoMessage};
pub use error::IsoError;
pub use parser::parse;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::AsciiTlvWriter;

    #[test]
    fn reject_unsupported_de65_on_build_and_parse() {
        let mut buf = [0u8; 256];
        let msg = IsoMessage::new(*b"0800").with(65, &[0x01]).unwrap();
        assert_eq!(build(&msg, &mut buf), Err(IsoError::UnsupportedField(65)));

        let mut wire = [0u8; 20];
        wire[0..4].copy_from_slice(b"0800");
        wire[4] = 0x80;  // primary bit 1 -> secondary bitmap present
        wire[12] = 0x80; // secondary bit 65 set
        assert_eq!(parse(&wire), Err(IsoError::UnsupportedField(65)));
    }

    #[test]
    fn reject_trailing_bytes() {
        let mut buf = [0u8; 256];
        let msg = IsoMessage::new(*b"0800")
            .with(de::STAN, b"123")
            .unwrap()
            .with(de::NETWORK_MGMT_INFO_CODE, b"301")
            .unwrap();
        let len = build(&msg, &mut buf).unwrap();

        buf[len] = 0xFF;
        assert_eq!(parse(&buf[..len + 1]), Err(IsoError::TrailingBytes { remaining: 1 }));
    }

    #[test]
    fn currency_a_or_n_validation() {
        let mut buf = [0u8; 64];

        let ok_num = IsoMessage::new(*b"0200").with(de::CURRENCY_TXN, b"978").unwrap();
        let len = build(&ok_num, &mut buf).unwrap();
        assert_eq!(parse(&buf[..len]).unwrap().get(de::CURRENCY_TXN), Some(&b"978"[..]));

        let ok_alpha = IsoMessage::new(*b"0200").with(de::CURRENCY_TXN, b"EUR").unwrap();
        let len = build(&ok_alpha, &mut buf).unwrap();
        assert_eq!(parse(&buf[..len]).unwrap().get(de::CURRENCY_TXN), Some(&b"EUR"[..]));

        let bad_mixed = IsoMessage::new(*b"0200").with(de::CURRENCY_TXN, b"9A8").unwrap();
        assert_eq!(
            build(&bad_mixed, &mut buf),
            Err(IsoError::InvalidCharacter { field: de::CURRENCY_TXN, byte: b'A' })
        );

        let bad_short = IsoMessage::new(*b"0200").with(de::CURRENCY_TXN, b"97").unwrap();
        assert_eq!(
            build(&bad_short, &mut buf),
            Err(IsoError::InvalidLength { field: de::CURRENCY_TXN, expected: 3, actual: 2 })
        );
    }

    #[test]
    fn message_ergonomics_and_tlv_bounds() {
        let mut msg = IsoMessage::new(*b"0200")
            .with(de::PROC_CODE, b"000000")
            .unwrap()
            .with(de::AMOUNT_TXN, b"656")
            .unwrap();

        assert!(msg.has(de::PROC_CODE));
        assert_eq!(msg.require(de::STAN), Err(IsoError::MissingField(de::STAN)));
        assert_eq!(msg.get_str(de::AMOUNT_TXN), Some(Ok("656")));

        msg.clear(de::PROC_CODE);
        assert!(!msg.has(de::PROC_CODE));
        assert_eq!((&msg).into_iter().count(), 1);

        let mut tlv_buf = [0u8; 128];
        let mut w = AsciiTlvWriter::new(de::ADD_DATA_PRIVATE, &mut tlv_buf);
        let oversized = [b'A'; 100];
        assert_eq!(
            w.push(b"01", &oversized),
            Err(IsoError::LengthExceedsMax { field: de::ADD_DATA_PRIVATE, max: 99, actual: 100 })
        );
    }
}