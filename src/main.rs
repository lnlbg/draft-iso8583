use draft_iso8583::composite::{AsciiTlvIter, AsciiTlvWriter, OriginalDataElements};
use draft_iso8583::{build, de, parse, IsoError, IsoMessage};

fn build_demo_tx(out_buf: &mut [u8]) -> Result<usize, IsoError> {
    let mut de48_buf = [0u8; 128];
    let de48_len = {
        let mut sw = AsciiTlvWriter::new(de::ADD_DATA_PRIVATE, &mut de48_buf);
        sw.push(b"01", b"DRAFT01")?;
        sw.push(b"02", b"TXN9999")?;
        sw.finish()
    };

    let mut de55_buf = [0u8; 128];
    let de55_len = {
        let mut sw = AsciiTlvWriter::new(de::ICC_DATA, &mut de55_buf);
        sw.push(b"9F", b"010203")?;
        sw.push(b"82", b"5800")?;
        sw.finish()
    };

    let mut de90_buf = [0u8; 42];
    OriginalDataElements::build_into(
        &mut de90_buf,
        b"0200",        // Original MTI
        b"000042",      // Original STAN
        b"0315123000",  // Original DateTime
        b"00000001234", // Original Acquiring ID
        b"00000005678", // Original Forwarding ID
    );

    let tx = IsoMessage::new(*b"9999")
        .with(de::PAN, b"4111111111111111")?
        .with(de::TRACK_2, b"4111111111111111=281210100000")?
        .with(de::RECEIVING_INST_ID, b"000123")?
        .with(de::PROC_CODE, b"0")?
        .with(de::AMOUNT_TXN, b"15000")?
        .with(de::STAN, b"42")?
        .with(de::ORIG_DATA_ELEMENTS, &de90_buf)?
        .with(de::FEE_TXN, b"C500")?
        .with(de::NET_SETTLEMENT_AMT, b"D125000")?
        .with(de::TID, b"TERM1")?
        .with(de::CURRENCY_TXN, b"978")?
        .with(de::ADD_RESP_DATA, b"APPROVED")?
        .with(de::ACCOUNT_ID_1, b"BG80BNBG96611020345678")?
        .with(de::ADD_DATA_PRIVATE, &de48_buf[..de48_len])?
        .with(de::ICC_DATA, &de55_buf[..de55_len])?
        .with(de::PIN_DATA, &[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0])?
        .with(de::MAC_EXTENDED, &[0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE])?;

    build(&tx, out_buf)
}

fn inspect_demo_tx(wire_packet: &[u8]) -> Result<(), IsoError> {
    let rx = parse(wire_packet)?;

    println!("Parsed MTI: {}", rx.mti_str().unwrap());
    println!("Populated fields count: {}", rx.iter().count());
    println!("DE 2  (PAN): {}", rx.get_str(de::PAN).unwrap().unwrap());
    println!("DE 4  (Amt): {}", rx.get_str(de::AMOUNT_TXN).unwrap().unwrap());
    println!("DE 28 (Fee): {}", rx.get_str(de::FEE_TXN).unwrap().unwrap());
    println!("DE 41 (TID): '{}'", rx.get_str(de::TID).unwrap().unwrap());
    println!("DE 97 (Net): {}", rx.get_str(de::NET_SETTLEMENT_AMT).unwrap().unwrap());
    println!("DE 102(Acc): {}", rx.get_str(de::ACCOUNT_ID_1).unwrap().unwrap());
    println!("DE 128(MAC): {:02X?}", rx.require(de::MAC_EXTENDED)?);

    let de90 = OriginalDataElements::parse(rx.require(de::ORIG_DATA_ELEMENTS)?)?;
    println!(
        "DE 90 subfields -> OrigMTI: {}, OrigSTAN: {}, OrigAcq: {}",
        core::str::from_utf8(de90.original_mti()).unwrap(),
        core::str::from_utf8(de90.original_stan()).unwrap(),
        core::str::from_utf8(de90.original_acquiring_id()).unwrap(),
    );

    println!("DE 48 subfields:");
    for item in AsciiTlvIter::new(de::ADD_DATA_PRIVATE, rx.require(de::ADD_DATA_PRIVATE)?) {
        let (tag, val) = item?;
        println!(
            "  Tag {} = {}",
            core::str::from_utf8(tag).unwrap(),
            core::str::from_utf8(val).unwrap()
        );
    }

    Ok(())
}

fn main() {
    let mut comm_buf = [0u8; 4096];

    let msg_len = build_demo_tx(&mut comm_buf).expect("build failed");
    println!("Built ISO8583 MTI 9999 ({} bytes):", msg_len);
    for b in &comm_buf[..msg_len] {
        print!("{:02X}", b);
    }
    println!("\n");

    inspect_demo_tx(&comm_buf[..msg_len]).expect("parse failed");
}