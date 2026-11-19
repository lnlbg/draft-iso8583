use draft_iso8583::{build, de, parse, IsoMessage};

fn print_hex(label: &str, bytes: &[u8]) {
    println!("{} ({} bytes):", label, bytes.len());
    for b in bytes {
        print!("{:02X}", b);
    }
    println!("\n");
}

fn main() {
    // Simulated channel buffers (Acquirer <-> Host/Switch)
    let mut tx_buf = [0u8; 512];
    let mut rx_buf = [0u8; 512];

    // ========================================================================
    // 1. CLIENT / ACQUIRER: Build 0800 Echo Test Request
    // ========================================================================
    let req = IsoMessage::new(*b"0800")
        .with(de::TRANSMISSION_DATETIME, b"0320143500").unwrap() // MMDDhhmmss
        .with(de::STAN, b"1").unwrap()                           // auto-padded to "000001"
        .with(de::NETWORK_MGMT_INFO_CODE, b"301").unwrap();      // 301 = Echo Test (secondary bitmap)

    let req_len = build(&req, &mut tx_buf).expect("failed to build 0800");
    print_hex("0800 Echo Request", &tx_buf[..req_len]);

    // ========================================================================
    // 2. HOST / SWITCH: Parse 0800 & Build 0810 Echo Test Response (zero-copy)
    // ========================================================================
    let host_rx = parse(&tx_buf[..req_len]).expect("host failed to parse 0800");
    assert_eq!(&host_rx.mti, b"0800");
    assert_eq!(host_rx.get(de::NETWORK_MGMT_INFO_CODE), Some(&b"301"[..]));

    // Echo DE 7, DE 11, DE 70 directly as zero-copy slices from host_rx
    let resp = IsoMessage::new(*b"0810")
        .with(de::TRANSMISSION_DATETIME, host_rx.require(de::TRANSMISSION_DATETIME).unwrap()).unwrap()
        .with(de::STAN, host_rx.require(de::STAN).unwrap()).unwrap()
        .with(de::RESPONSE_CODE, b"00").unwrap() // "00" = Approved / OK
        .with(de::NETWORK_MGMT_INFO_CODE, host_rx.require(de::NETWORK_MGMT_INFO_CODE).unwrap()).unwrap();

    let resp_len = build(&resp, &mut rx_buf).expect("host failed to build 0810");
    print_hex("0810 Echo Response", &rx_buf[..resp_len]);

    // ========================================================================
    // 3. CLIENT / ACQUIRER: Parse 0810 & Verify Echo Match
    // ========================================================================
    let client_rx = parse(&rx_buf[..resp_len]).expect("client failed to parse 0810");

    assert_eq!(&client_rx.mti, b"0810");
    assert_eq!(client_rx.get(de::TRANSMISSION_DATETIME), Some(&b"0320143500"[..]));
    assert_eq!(client_rx.get(de::STAN), Some(&b"000001"[..]));
    assert_eq!(client_rx.get(de::RESPONSE_CODE), Some(&b"00"[..]));
    assert_eq!(client_rx.get(de::NETWORK_MGMT_INFO_CODE), Some(&b"301"[..]));

    println!(
        "Echo Test OK -> MTI: {}, DE7: {}, DE11: {}, DE39: {}, DE70: {}",
        client_rx.mti_str().unwrap(),
        client_rx.get_str(de::TRANSMISSION_DATETIME).unwrap().unwrap(),
        client_rx.get_str(de::STAN).unwrap().unwrap(),
        client_rx.get_str(de::RESPONSE_CODE).unwrap().unwrap(),
        client_rx.get_str(de::NETWORK_MGMT_INFO_CODE).unwrap().unwrap(),
    );
}