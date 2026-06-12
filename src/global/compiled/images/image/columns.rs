use crate::global::compiled::lowering::CompiledProgramImage;

pub(crate) const PROGRAM_IMAGE_ATOM_STRIDE: usize = 8;
pub(crate) const PROGRAM_IMAGE_POLICY_STRIDE: usize = 8;
pub(crate) const PROGRAM_IMAGE_CONTROL_DESC_STRIDE: usize = 12;
pub(crate) const PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE: usize = 12;
pub(crate) const PROGRAM_IMAGE_NO_ROUTE_CONTROLLER: u8 = u8::MAX;
pub(crate) const PROGRAM_IMAGE_SUBJECT_NONE: u8 = u8::MAX;
pub(crate) const PROGRAM_IMAGE_SUBJECT_ROUTE_ARM: u8 = 0;
pub(crate) const PROGRAM_IMAGE_SUBJECT_LOOP_CONTINUE: u8 = 1;
pub(crate) const PROGRAM_IMAGE_SUBJECT_LOOP_BREAK: u8 = 2;

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
            panic!("program image");
        }
        if stride == 0 {
            panic!("program image");
        }
        let byte_len = len * stride;
        if byte_len > (u16::MAX as usize - offset) {
            panic!("program image");
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
    pub(crate) policies: ProgramColumnRange,
    pub(crate) control_descs: ProgramColumnRange,
    pub(crate) route_controls: ProgramColumnRange,
}

impl ProgramImageColumns {
    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        let mut len = self.atoms.end_offset(PROGRAM_IMAGE_ATOM_STRIDE);
        if self.policies.end_offset(PROGRAM_IMAGE_POLICY_STRIDE) > len {
            len = self.policies.end_offset(PROGRAM_IMAGE_POLICY_STRIDE);
        }
        if self
            .control_descs
            .end_offset(PROGRAM_IMAGE_CONTROL_DESC_STRIDE)
            > len
        {
            len = self
                .control_descs
                .end_offset(PROGRAM_IMAGE_CONTROL_DESC_STRIDE);
        }
        if self
            .route_controls
            .end_offset(PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE)
            > len
        {
            len = self
                .route_controls
                .end_offset(PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE);
        }
        len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramImageFacts {
    pub(crate) role_count: u8,
    pub(crate) control_scope_mask: u8,
}

impl ProgramImageFacts {
    #[inline(always)]
    pub(crate) const fn from_image(image: &CompiledProgramImage) -> Self {
        Self {
            role_count: image.compiled_program_role_count() as u8,
            control_scope_mask: image.compiled_program_control_scope_mask(),
        }
    }
}
