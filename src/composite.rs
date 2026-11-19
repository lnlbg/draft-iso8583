use crate::builder::BufWriter;
use crate::error::IsoError;

/// Zero-copy view over DE 90 (Original Data Elements, 42 fixed bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginalDataElements<'a> {
    raw: &'a [u8; 42],
}

impl<'a> OriginalDataElements<'a> {
    pub fn parse(slice: &'a [u8]) -> Result<Self, IsoError> {
        let raw: &'a [u8; 42] = slice
            .try_into()
            .map_err(|_| IsoError::InvalidComposite { field: 90 })?;
        Ok(Self { raw })
    }

    #[must_use]
    pub fn original_mti(&self) -> &'a [u8] { &self.raw[0..4] }
    #[must_use]
    pub fn original_stan(&self) -> &'a [u8] { &self.raw[4..10] }
    #[must_use]
    pub fn original_datetime(&self) -> &'a [u8] { &self.raw[10..20] }
    #[must_use]
    pub fn original_acquiring_id(&self) -> &'a [u8] { &self.raw[20..31] }
    #[must_use]
    pub fn original_forwarding_id(&self) -> &'a [u8] { &self.raw[31..42] }

    /// Writes DE 90 subfields into a caller-provided 42-byte stack buffer.
    pub fn build_into(
        out: &mut [u8; 42],
        mti: &[u8; 4],
        stan: &[u8; 6],
        datetime: &[u8; 10],
        acq_id: &[u8; 11],
        fwd_id: &[u8; 11],
    ) {
        out[0..4].copy_from_slice(mti);
        out[4..10].copy_from_slice(stan);
        out[10..20].copy_from_slice(datetime);
        out[20..31].copy_from_slice(acq_id);
        out[31..42].copy_from_slice(fwd_id);
    }
}

/// Zero-allocation application-layer writer for ASCII TLV subfields:
/// `[Tag: 2B][Len: 2B ASCII][Value: Len B]`.
pub struct AsciiTlvWriter<'a> {
    field_id: u8,
    w: BufWriter<'a>,
}

impl<'a> AsciiTlvWriter<'a> {
    pub fn new(field_id: u8, buf: &'a mut [u8]) -> Self {
        Self { field_id, w: BufWriter::new(buf) }
    }

    pub fn push(&mut self, tag: &[u8; 2], value: &[u8]) -> Result<(), IsoError> {
        if value.len() > 99 {
            return Err(IsoError::LengthExceedsMax {
                field: self.field_id,
                max: 99,
                actual: value.len(),
            });
        }
        let len_ascii = [((value.len() / 10) as u8) + b'0', ((value.len() % 10) as u8) + b'0'];
        self.w.write_bytes(tag)?;
        self.w.write_bytes(&len_ascii)?;
        self.w.write_bytes(value)
    }

    #[must_use]
    pub fn finish(self) -> usize {
        self.w.written()
    }
}

/// Zero-copy application-layer iterator over ASCII TLV subfields inside composite DEs (e.g. DE 48).
pub struct AsciiTlvIter<'a> {
    field_id: u8,
    rest: &'a [u8],
}

impl<'a> AsciiTlvIter<'a> {
    #[must_use]
    pub fn new(field_id: u8, data: &'a [u8]) -> Self {
        Self { field_id, rest: data }
    }
}

impl<'a> Iterator for AsciiTlvIter<'a> {
    type Item = Result<(&'a [u8; 2], &'a [u8]), IsoError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        if self.rest.len() < 4 {
            self.rest = &[];
            return Some(Err(IsoError::InvalidComposite { field: self.field_id }));
        }
        let tag: &'a [u8; 2] = self.rest[0..2].try_into().ok()?;
        let d0 = self.rest[2];
        let d1 = self.rest[3];
        if !d0.is_ascii_digit() || !d1.is_ascii_digit() {
            self.rest = &[];
            return Some(Err(IsoError::InvalidComposite { field: self.field_id }));
        }
        let len = ((d0 - b'0') as usize) * 10 + (d1 - b'0') as usize;
        if self.rest.len() < 4 + len {
            self.rest = &[];
            return Some(Err(IsoError::InvalidComposite { field: self.field_id }));
        }
        let val = &self.rest[4..4 + len];
        self.rest = &self.rest[4 + len..];
        Some(Ok((tag, val)))
    }
}