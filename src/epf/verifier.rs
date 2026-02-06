use core::convert::TryInto;

/// VM header as laid out in bytecode (after the four-byte magic).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub code_len: u16,
    pub fuel_max: u16,
    pub mem_len: u16,
    pub flags: u16,
    pub hash: u32,
}

impl Header {
    pub const MAGIC: [u8; 4] = *b"K1VM";
    pub const SIZE: usize = 4 + 2 + 2 + 2 + 2 + 4;

    pub const fn max_mem_len() -> usize {
        1024
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, VerifyError> {
        if bytes.len() < Self::SIZE {
            return Err(VerifyError::TooShort);
        }
        if bytes[..4] != Self::MAGIC {
            return Err(VerifyError::BadMagic);
        }
        let code_len = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        let fuel_max = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
        let mem_len = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let flags = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
        let hash = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        if mem_len as usize > Self::max_mem_len() {
            return Err(VerifyError::MemTooLarge { requested: mem_len });
        }
        if fuel_max == 0 {
            return Err(VerifyError::ZeroFuel);
        }
        Ok(Self {
            code_len,
            fuel_max,
            mem_len,
            flags,
            hash,
        })
    }

    pub fn encode_into(&self, buf: &mut [u8; Self::SIZE]) {
        buf[..4].copy_from_slice(&Self::MAGIC);
        buf[4..6].copy_from_slice(&self.code_len.to_le_bytes());
        buf[6..8].copy_from_slice(&self.fuel_max.to_le_bytes());
        buf[8..10].copy_from_slice(&self.mem_len.to_le_bytes());
        buf[10..12].copy_from_slice(&self.flags.to_le_bytes());
        buf[12..16].copy_from_slice(&self.hash.to_le_bytes());
    }
}

/// Verification failures raised while loading a bytecode image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    TooShort,
    BadMagic,
    CodeLengthMismatch { declared: u16, actual: usize },
    CodeTooLarge { declared: u16 },
    MemTooLarge { requested: u16 },
    HashMismatch { expected: u32, computed: u32 },
    ZeroFuel,
}

/// Fully verified bytecode image (header + code slice).
#[derive(Clone, Copy)]
pub struct VerifiedImage<'a> {
    pub header: Header,
    pub code: &'a [u8],
}

impl<'a> core::fmt::Debug for VerifiedImage<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VerifiedImage")
            .field("header", &self.header)
            .field("code_len", &self.code.len())
            .finish()
    }
}

impl<'a> VerifiedImage<'a> {
    pub const MAX_CODE_LEN: usize = 2048;

    pub fn new(bytes: &'a [u8]) -> Result<Self, VerifyError> {
        if bytes.len() < Header::SIZE {
            return Err(VerifyError::TooShort);
        }
        let header = Header::parse(bytes)?;
        let code_len = header.code_len as usize;
        if code_len > Self::MAX_CODE_LEN {
            return Err(VerifyError::CodeTooLarge {
                declared: header.code_len,
            });
        }
        let remaining = bytes.len().saturating_sub(Header::SIZE);
        if remaining != code_len {
            return Err(VerifyError::CodeLengthMismatch {
                declared: header.code_len,
                actual: remaining,
            });
        }
        let code = &bytes[Header::SIZE..];
        let hash = compute_hash(code);
        if hash != header.hash {
            return Err(VerifyError::HashMismatch {
                expected: header.hash,
                computed: hash,
            });
        }
        Ok(Self { header, code })
    }
}

/// Deterministic 32-bit hash (FNV-1a) used for bytecode integrity.
pub fn compute_hash(code: &[u8]) -> u32 {
    const OFFSET: u32 = 0x811C_9DC5;
    const PRIME: u32 = 0x0100_0193;
    let mut hash = OFFSET;
    for byte in code {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_roundtrip() {
        let code = [0xFFu8, 0x00, 0x00, 0x00];
        let mut image = [0u8; Header::SIZE + 4];
        let hash = compute_hash(&code);
        let header = Header {
            code_len: code.len() as u16,
            fuel_max: 16,
            mem_len: 32,
            flags: 0,
            hash,
        };
        header.encode_into((&mut image[..Header::SIZE]).try_into().unwrap());
        image[Header::SIZE..].copy_from_slice(&code);

        let verified = VerifiedImage::new(&image).expect("must verify");
        assert_eq!(verified.header.fuel_max, 16);
        assert_eq!(verified.code, code);
    }

    #[test]
    fn reject_bad_magic() {
        let mut bytes = [0u8; Header::SIZE];
        bytes[..4].copy_from_slice(b"BAD!");
        assert!(matches!(
            VerifiedImage::new(&bytes).unwrap_err(),
            VerifyError::BadMagic
        ));
    }

    #[test]
    fn reject_hash_mismatch() {
        let mut image = [0u8; Header::SIZE + 2];
        let header = Header {
            code_len: 2,
            fuel_max: 8,
            mem_len: 32,
            flags: 0,
            hash: 0xDEAD_BEEF,
        };
        header.encode_into((&mut image[..Header::SIZE]).try_into().unwrap());
        image[Header::SIZE..].copy_from_slice(&[1, 2]);
        assert!(matches!(
            VerifiedImage::new(&image).unwrap_err(),
            VerifyError::HashMismatch { .. }
        ));
    }
}
