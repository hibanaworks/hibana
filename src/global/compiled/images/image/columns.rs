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

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramImageColumn {
    pub(crate) offset: u16,
    pub(crate) len: u16,
    pub(crate) stride: u8,
}

impl ProgramImageColumn {
    pub(crate) const EMPTY: Self = Self {
        offset: 0,
        len: 0,
        stride: 1,
    };

    #[inline(always)]
    pub(crate) const fn new(offset: usize, len: usize, stride: usize) -> Self {
        if offset > u16::MAX as usize || len > u16::MAX as usize || stride > u8::MAX as usize {
            panic!("program image");
        }
        if stride == 0 {
            panic!("program image");
        }
        if offset.saturating_add(len.saturating_mul(stride)) > u16::MAX as usize {
            panic!("program image");
        }
        Self {
            offset: offset as u16,
            len: len as u16,
            stride: stride as u8,
        }
    }

    #[inline(always)]
    pub(crate) const fn byte_len(self) -> usize {
        self.len as usize * self.stride as usize
    }

    #[inline(always)]
    pub(crate) const fn end_offset(self) -> usize {
        self.offset as usize + self.byte_len()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramImageColumns {
    pub(crate) atoms: ProgramImageColumn,
    pub(crate) policies: ProgramImageColumn,
    pub(crate) control_descs: ProgramImageColumn,
    pub(crate) route_controls: ProgramImageColumn,
}

impl ProgramImageColumns {
    #[inline(always)]
    pub(crate) const fn empty() -> Self {
        Self {
            atoms: ProgramImageColumn::EMPTY,
            policies: ProgramImageColumn::EMPTY,
            control_descs: ProgramImageColumn::EMPTY,
            route_controls: ProgramImageColumn::EMPTY,
        }
    }

    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        let mut len = self.atoms.end_offset();
        if self.policies.end_offset() > len {
            len = self.policies.end_offset();
        }
        if self.control_descs.end_offset() > len {
            len = self.control_descs.end_offset();
        }
        if self.route_controls.end_offset() > len {
            len = self.route_controls.end_offset();
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
        let role_count = image.compiled_program_role_count();
        if role_count > u8::MAX as usize {
            panic!("program image");
        }
        Self {
            role_count: role_count as u8,
            control_scope_mask: image.compiled_program_control_scope_mask(),
        }
    }
}
