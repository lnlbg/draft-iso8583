# draft-iso8583

**Version ISO 8583:1987 (ASCII + Raw Binary) 100%**

A zero-copy, zero-allocation, `no_std`-compatible ISO 8583:1987 message parser and builder written in Rust.

> **Note:** The architecture, data structures, and algorithm design are human-authored; the implementation was produced with AI assistance, adhering to Rust low-level and embedded (`no_std`) coding standards.

## Features

- **100% ISO 8583:1987 (ASCII + Raw Binary)** field table (`DE 2..=128`) with primary and secondary bitmap support.
- **`no_std` & Zero Heap Allocation:** Uses a stack-allocated fixed array (`[Option<&'a [u8]>; 129]`) with `O(1)` field access and automatic ascending canonical ordering during serialization.
- **Zero-Copy Parsing:** `parse()` borrows field slices directly from the incoming wire buffer without copying or allocating.
- **Strict Validation:**
  - MTI validation (4 ASCII digits).
  - Rejection of unsupported `DE 65` (extended bitmap bit).
  - Exact trailing-bytes check (`TrailingBytes`).
  - Per-field length, prefix (`LLVAR`/`LLLVAR`), sign (`C`/`D` for `x+n`), and character-set validation (`Numeric`, `NumSpec`, `Track2`, `AlphaNum`, `AlphaOrNum`, `AlphaNumSpec`, `Binary`).
- **Composite Helpers:** Zero-copy `OriginalDataElements` (`DE 90`) and `AsciiTlvWriter` / `AsciiTlvIter` for ASCII TLV subfields (e.g. `DE 48`, `DE 55`).

## Requirements

- Rust **1.85+** (Rust Edition 2024)

## Build & Test

```bash
cargo test
cargo test --no-default-features
cargo test --lib --no-default-features
cargo run
cargo run --example echo
```

## Quick Example

```rust
use draft_iso8583::{build, de, parse, IsoMessage};

fn main() -> Result<(), draft_iso8583::IsoError> {
    let mut tx_buf = [0u8; 512];

    let req = IsoMessage::new(*b"0800")
        .with(de::TRANSMISSION_DATETIME, b"0320143500")?
        .with(de::STAN, b"1")? // auto-padded to "000001" on wire
        .with(de::NETWORK_MGMT_INFO_CODE, b"301")?;

    let len = build(&req, &mut tx_buf)?;
    let rx = parse(&tx_buf[..len])?;

    assert_eq!(&rx.mti, b"0800");
    assert_eq!(rx.get(de::STAN), Some(&b"000001"[..]));
    Ok(())
}
```

## Project Structure

- `src/lib.rs` — Crate root (`no_std` gate), public exports, and integration tests
- `src/builder.rs` — `IsoMessage`, `BufWriter`, and wire serializer `build()`
- `src/parser.rs` — `BufReader` and zero-copy wire parser `parse()`
- `src/handlers.rs` — Table-driven `FIELD_TABLE` (`DE 0..=128`) and wire format handlers
- `src/composite.rs` — `OriginalDataElements` (`DE 90`) and `AsciiTlvWriter` / `AsciiTlvIter`
- `src/de.rs` — Standard ISO 8583:1987 Data Element constants
- `src/error.rs` — `IsoError` definitions
- `src/main.rs` — Demo application (`app`)
- `examples/echo.rs` — 0800/0810 Echo Test example

## License

Licensed under either of:
- MIT / Apache-2.0
