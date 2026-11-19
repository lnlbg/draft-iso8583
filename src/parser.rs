use crate::builder::IsoMessage;
use crate::error::IsoError;
use crate::handlers::FIELD_TABLE;

pub(crate) struct BufReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> BufReader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline]
    pub(crate) fn read_exact(&mut self, len: usize) -> Result<&'a [u8], IsoError> {
        let end = self.pos.checked_add(len).ok_or(IsoError::UnexpectedEof)?;
        if end > self.buf.len() {
            return Err(IsoError::UnexpectedEof);
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    #[inline]
    pub(crate) fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
}

#[inline]
fn is_bit_set(bitmap: &[u8], bit_1_based: usize) -> bool {
    let idx = (bit_1_based - 1) / 8;
    let shift = 7 - ((bit_1_based - 1) % 8);
    (bitmap[idx] & (1 << shift)) != 0
}

/// Zero-copy parser from the wire buffer; requires the entire `in_buf` slice to be consumed.
pub fn parse<'a>(in_buf: &'a [u8]) -> Result<IsoMessage<'a>, IsoError> {
    let mut r = BufReader::new(in_buf);

    let mti_bytes = r.read_exact(4)?;
    if !mti_bytes.iter().all(|b| b.is_ascii_digit()) {
        return Err(IsoError::InvalidMti);
    }
    let mti: [u8; 4] = mti_bytes.try_into().map_err(|_| IsoError::InvalidMti)?;
    let mut msg = IsoMessage::new(mti);

    let primary = r.read_exact(8)?;
    let has_secondary = is_bit_set(primary, 1);

    let secondary = if has_secondary {
        Some(r.read_exact(8)?)
    } else {
        None
    };

    for id in 2..=64usize {
        if is_bit_set(primary, id) {
            let spec = &FIELD_TABLE[id];
            let slice = (spec.parse)(spec, &mut r)?;
            msg.set(id as u8, slice)?;
        }
    }

    if let Some(sec) = secondary {
        for id in 65..=128usize {
            if is_bit_set(sec, id - 64) {
                let spec = &FIELD_TABLE[id];
                let slice = (spec.parse)(spec, &mut r)?;
                msg.set(id as u8, slice)?;
            }
        }
    }

    let remaining = r.remaining();
    if remaining != 0 {
        return Err(IsoError::TrailingBytes { remaining });
    }

    Ok(msg)
}