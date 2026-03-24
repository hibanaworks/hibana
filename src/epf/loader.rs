use super::{
    Slot,
    verifier::{Header, VerifiedImage, VerifyError, compute_hash},
};

/// Errors surfaced by the image loader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoaderError {
    AlreadyLoading,
    NotLoading,
    CodeTooLarge { declared: u16 },
    UnexpectedOffset { expected: u32, got: u32 },
    ChunkTooLarge { remaining: u32, provided: u32 },
    HashMismatch { expected: u32, computed: u32 },
    Verify(VerifyError),
}

/// Simple staging loader for EPF VM bytecode images.
///
/// The loader accepts a [`Header`] upfront (`begin`), followed by a series of
/// `write` calls that must stream the code in-order. Once all bytes are present,
/// `commit` validates the hash and returns a [`VerifiedImage`] view.
#[derive(Clone, Debug)]
pub(crate) struct ImageLoader {
    header: Option<Header>,
    buffer: [u8; verify_buffer_len()],
    written: u32,
}

const fn verify_buffer_len() -> usize {
    super::verifier::VerifiedImage::MAX_CODE_LEN
}

impl ImageLoader {
    /// Construct an empty loader.
    pub(crate) const fn new() -> Self {
        Self {
            header: None,
            buffer: [0; verify_buffer_len()],
            written: 0,
        }
    }

    /// Begin a new load sequence.
    pub(crate) fn begin(&mut self, header: Header) -> Result<(), LoaderError> {
        if self.header.is_some() {
            return Err(LoaderError::AlreadyLoading);
        }
        if header.code_len as usize > self.buffer.len() {
            return Err(LoaderError::CodeTooLarge {
                declared: header.code_len,
            });
        }
        self.header = Some(header);
        self.written = 0;
        Ok(())
    }

    /// Append a sequential chunk at the given offset.
    pub(crate) fn write(&mut self, offset: u32, chunk: &[u8]) -> Result<(), LoaderError> {
        let header = self.header.ok_or(LoaderError::NotLoading)?;
        if offset != self.written {
            return Err(LoaderError::UnexpectedOffset {
                expected: self.written,
                got: offset,
            });
        }
        let remaining = header.code_len as u32 - self.written;
        if chunk.len() as u32 > remaining {
            return Err(LoaderError::ChunkTooLarge {
                remaining,
                provided: chunk.len() as u32,
            });
        }
        let start = self.written as usize;
        let end = start + chunk.len();
        self.buffer[start..end].copy_from_slice(chunk);
        self.written += chunk.len() as u32;
        Ok(())
    }

    /// Finalise the load, validating the hash and returning a verified view.
    #[cfg(test)]
    pub(crate) fn commit(&mut self) -> Result<VerifiedImage<'_>, LoaderError> {
        self.commit_inner(None)
    }

    /// Finalise the load with slot-specific verification rules.
    pub(crate) fn commit_for_slot(&mut self, slot: Slot) -> Result<VerifiedImage<'_>, LoaderError> {
        self.commit_inner(Some(slot))
    }

    fn commit_inner(&mut self, slot: Option<Slot>) -> Result<VerifiedImage<'_>, LoaderError> {
        let header = self.header.take().ok_or(LoaderError::NotLoading)?;
        if self.written != header.code_len as u32 {
            // Treat short write as a length mismatch from the verifier.
            self.header = Some(header);
            return Err(LoaderError::Verify(VerifyError::CodeLengthMismatch {
                declared: header.code_len,
                actual: self.written as usize,
            }));
        }
        let code = &self.buffer[..header.code_len as usize];
        let hash = compute_hash(code);
        if hash != header.hash {
            self.header = Some(header);
            return Err(LoaderError::HashMismatch {
                expected: header.hash,
                computed: hash,
            });
        }
        // Construct a byte image (header + code) so we can leverage the verifier
        // for the final safety net. The header is small enough that we can build
        // a stack buffer.
        let mut header_bytes = [0u8; Header::SIZE];
        header.encode_into(&mut header_bytes);
        let mut image = [0u8; Header::SIZE + verify_buffer_len()];
        image[..Header::SIZE].copy_from_slice(&header_bytes);
        image[Header::SIZE..Header::SIZE + code.len()].copy_from_slice(code);
        let verified = match slot {
            Some(slot) => VerifiedImage::new_for_slot(&image[..Header::SIZE + code.len()], slot),
            None => VerifiedImage::new(&image[..Header::SIZE + code.len()]),
        }
        .map_err(LoaderError::Verify)?;
        Ok(VerifiedImage {
            header: verified.header,
            code: &self.buffer[..header.code_len as usize],
        })
    }
}

impl Default for ImageLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_header(code: &[u8]) -> Header {
        Header {
            code_len: code.len() as u16,
            fuel_max: 8,
            mem_len: 32,
            flags: 0,
            hash: compute_hash(code),
        }
    }

    #[test]
    fn sequential_load_and_commit() {
        let code = [0x01u8, 0x02, 0x03, 0x04];
        let header = build_header(&code);
        let mut loader = ImageLoader::new();
        loader.begin(header).unwrap();
        loader.write(0, &code[..2]).unwrap();
        loader.write(2, &code[2..]).unwrap();
        let verified = loader.commit().expect("commit succeeds");
        assert_eq!(verified.code, code);
        assert_eq!(verified.header.hash, header.hash);
    }

    #[test]
    fn reject_offset_mismatch() {
        let code = [0xAA, 0xBB];
        let header = build_header(&code);
        let mut loader = ImageLoader::new();
        loader.begin(header).unwrap();
        let err = loader.write(1, &code).unwrap_err();
        assert!(matches!(err, LoaderError::UnexpectedOffset { .. }));
    }

    #[test]
    fn reject_hash_mismatch_on_commit() {
        let mut loader = ImageLoader::new();
        let header = Header {
            code_len: 2,
            fuel_max: 4,
            mem_len: 8,
            flags: 0,
            hash: 0x1234_5678,
        };
        loader.begin(header).unwrap();
        loader.write(0, &[0x00, 0x01]).unwrap();
        let err = loader.commit().unwrap_err();
        assert!(matches!(err, LoaderError::HashMismatch { .. }));
    }

    #[test]
    fn reject_get_input_when_slot_contract_forbids_it() {
        let code = [
            super::super::ops::instr::GET_INPUT,
            0x00,
            0x00,
            super::super::ops::instr::HALT,
        ];
        let header = build_header(&code);
        let mut loader = ImageLoader::new();
        loader.begin(header).unwrap();
        loader.write(0, &code).unwrap();

        let err = loader.commit_for_slot(Slot::Forward).unwrap_err();
        assert!(matches!(
            err,
            LoaderError::Verify(VerifyError::InputForbiddenForSlot {
                slot: Slot::Forward,
                ..
            })
        ));
    }
}
