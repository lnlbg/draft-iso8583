use crate::error::IsoError;
use crate::handlers::FIELD_TABLE;

/// Stack-allocated zero-copy container for outbound and parsed fields (DE 2..=128).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoMessage<'a> {
    pub mti: [u8; 4],
    fields: [Option<&'a [u8]>; 129],
}

impl<'a> IsoMessage<'a> {
    #[must_use]
    pub const fn new(mti: [u8; 4]) -> Self {
        Self { mti, fields: [None; 129] }
    }

    pub fn set(&mut self, id: u8, value: &'a [u8]) -> Result<(), IsoError> {
        if !(2..=128).contains(&id) {
            return Err(IsoError::InvalidFieldId(id));
        }
        self.fields[id as usize] = Some(value);
        Ok(())
    }

    /// Chainable builder helper for populating fields.
    pub fn with(mut self, id: u8, value: &'a [u8]) -> Result<Self, IsoError> {
        self.set(id, value)?;
        Ok(self)
    }

    #[must_use]
    #[inline]
    pub fn get(&self, id: u8) -> Option<&'a [u8]> {
        if (2..=128).contains(&id) {
            self.fields[id as usize]
        } else {
            None
        }
    }

    #[must_use]
    #[inline]
    pub fn has(&self, id: u8) -> bool {
        self.get(id).is_some()
    }

    pub fn clear(&mut self, id: u8) {
        if (2..=128).contains(&id) {
            self.fields[id as usize] = None;
        }
    }

    /// Returns the field slice or `IsoError::MissingField(id)` if absent.
    #[inline]
    pub fn require(&self, id: u8) -> Result<&'a [u8], IsoError> {
        if !(2..=128).contains(&id) {
            return Err(IsoError::InvalidFieldId(id));
        }
        self.fields[id as usize].ok_or(IsoError::MissingField(id))
    }

    #[must_use]
    #[inline]
    pub fn get_str(&self, id: u8) -> Option<Result<&'a str, core::str::Utf8Error>> {
        self.get(id).map(core::str::from_utf8)
    }

    #[inline]
    pub fn mti_str(&self) -> Result<&str, core::str::Utf8Error> {
        core::str::from_utf8(&self.mti)
    }

    #[must_use]
    pub fn has_secondary_bitmap(&self) -> bool {
        self.fields[65..=128].iter().any(|f| f.is_some())
    }

    /// Iterates over all populated fields in ascending order `(field_id, slice)`.
    #[must_use]
    pub fn iter(&self) -> FieldIter<'_, 'a> {
        FieldIter { fields: &self.fields, idx: 2 }
    }
}

impl<'m, 'a> IntoIterator for &'m IsoMessage<'a> {
    type Item = (u8, &'a [u8]);
    type IntoIter = FieldIter<'m, 'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct FieldIter<'m, 'a> {
    fields: &'m [Option<&'a [u8]>; 129],
    idx: usize,
}

impl<'m, 'a> Iterator for FieldIter<'m, 'a> {
    type Item = (u8, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        while self.idx <= 128 {
            let id = self.idx;
            self.idx += 1;
            if let Some(val) = self.fields[id] {
                return Some((id as u8, val));
            }
        }
        None
    }
}

pub(crate) struct BufWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BufWriter<'a> {
    pub(crate) fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline]
    pub(crate) fn write_bytes(&mut self, data: &[u8]) -> Result<(), IsoError> {
        let end = self.pos.checked_add(data.len()).ok_or(IsoError::BufferTooSmall)?;
        if end > self.buf.len() {
            return Err(IsoError::BufferTooSmall);
        }
        self.buf[self.pos..end].copy_from_slice(data);
        self.pos = end;
        Ok(())
    }

    /// Writes `count` copies of `byte` directly into the output buffer (zero-alloc padding).
    #[inline]
    pub(crate) fn write_repeat(&mut self, byte: u8, count: usize) -> Result<(), IsoError> {
        let end = self.pos.checked_add(count).ok_or(IsoError::BufferTooSmall)?;
        if end > self.buf.len() {
            return Err(IsoError::BufferTooSmall);
        }
        self.buf[self.pos..end].fill(byte);
        self.pos = end;
        Ok(())
    }

    #[inline]
    pub(crate) fn written(&self) -> usize {
        self.pos
    }
}

#[inline]
fn set_bitmap_bit(bitmap: &mut [u8], bit_1_based: usize) {
    let idx = (bit_1_based - 1) / 8;
    let shift = 7 - ((bit_1_based - 1) % 8);
    bitmap[idx] |= 1 << shift;
}

/// Serializes an ISO 8583 message directly into `out_buf`, returning bytes written.
pub fn build(msg: &IsoMessage<'_>, out_buf: &mut [u8]) -> Result<usize, IsoError> {
    if !msg.mti.iter().all(|b| b.is_ascii_digit()) {
        return Err(IsoError::InvalidMti);
    }

    let has_secondary = msg.has_secondary_bitmap();
    let mut primary = [0u8; 8];
    let mut secondary = [0u8; 8];

    if has_secondary {
        set_bitmap_bit(&mut primary, 1);
    }

    for id in 2..=64usize {
        if msg.fields[id].is_some() {
            set_bitmap_bit(&mut primary, id);
        }
    }
    if has_secondary {
        for id in 65..=128usize {
            if msg.fields[id].is_some() {
                set_bitmap_bit(&mut secondary, id - 64);
            }
        }
    }

    let mut w = BufWriter::new(out_buf);
    w.write_bytes(&msg.mti)?;
    w.write_bytes(&primary)?;
    if has_secondary {
        w.write_bytes(&secondary)?;
    }

    let max_id = if has_secondary { 128 } else { 64 };
    for id in 2..=max_id {
        if let Some(val) = msg.fields[id] {
            let spec = &FIELD_TABLE[id];
            (spec.build)(spec, val, &mut w)?;
        }
    }

    Ok(w.written())
}