use crate::global::compiled::lowering::CompiledProgramImage;

pub(crate) const PROGRAM_IMAGE_ATOM_STRIDE: usize = 7;
pub(crate) const PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 6;
pub(crate) const PROGRAM_IMAGE_INTRINSIC_ROUTE_ROLE: u8 = u8::MAX;
pub(crate) const PROGRAM_IMAGE_INTRINSIC_ROUTE_DECISION_TAG: u8 = 0;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramColumnRange {
    pub(crate) offset: u16,
    pub(crate) len: u16,
}

impl ProgramColumnRange {
    #[inline(always)]
    pub(crate) const fn new(offset: usize, len: usize, stride: usize) -> Self {
        if offset > u16::MAX as usize || len > u16::MAX as usize {
            crate::invariant();
        }
        if stride == 0 {
            crate::invariant();
        }
        let byte_len = len * stride;
        if byte_len > (u16::MAX as usize - offset) {
            crate::invariant();
        }
        Self {
            offset: offset as u16,
            len: len as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn byte_len(self, stride: usize) -> usize {
        self.len as usize * stride
    }

    #[inline(always)]
    pub(crate) const fn end_offset(self, stride: usize) -> usize {
        self.offset as usize + self.byte_len(stride)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramImageColumns {
    pub(crate) atoms: ProgramColumnRange,
    pub(crate) route_resolvers: ProgramColumnRange,
}

impl ProgramImageColumns {
    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        let mut len = self.atoms.end_offset(PROGRAM_IMAGE_ATOM_STRIDE);
        if self
            .route_resolvers
            .end_offset(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE)
            > len
        {
            len = self
                .route_resolvers
                .end_offset(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE);
        }
        len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramImageFacts {
    pub(crate) role_count: u8,
}

impl ProgramImageFacts {
    #[inline(always)]
    pub(crate) const fn from_image(image: &CompiledProgramImage) -> Self {
        Self {
            role_count: image.compiled_program_role_count() as u8,
        }
    }
}
