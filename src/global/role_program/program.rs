use super::{
    BuildProgramSource, CompiledProgramImage, Program, ProgramStamp, RoleFacts, RoleImage,
    RoleImageRef, RoleImageSource, RoleLaneImage, RoleProgramView, private,
    validated_program_image,
};
struct ValidatedRoleImage<Steps, const ROLE: u8>(core::marker::PhantomData<Steps>);

impl<Steps, const ROLE: u8> ValidatedRoleImage<Steps, ROLE>
where
    Steps: BuildProgramSource,
{
    fn program_image() -> &'static CompiledProgramImage {
        validated_program_image::<Steps>()
    }

    const STAMP: ProgramStamp = validated_program_image::<Steps>().stamp();
    const FACTS: RoleFacts =
        RoleFacts::from_counts(validated_program_image::<Steps>().role_lowering_counts::<ROLE>());
    const LANES: RoleLaneImage = RoleLaneImage::from_program::<ROLE>(
        validated_program_image::<Steps>(),
        Self::FACTS.footprint().logical_lane_count,
    );
    const IMAGE: RoleImage = RoleImage::new(
        Self::FACTS,
        RoleImageSource::new(Self::program_image),
        Self::LANES,
    );
    const COMPILED_IMAGE: crate::global::compiled::images::CompiledRoleImage =
        crate::global::compiled::images::CompiledRoleImage::new(
            crate::global::compiled::images::CompiledProgramRef::resident(
                Self::STAMP,
                validated_program_image::<Steps>(),
            ),
            ROLE,
            RoleImageRef::new(&Self::IMAGE),
        );
}

pub struct RoleProgram<const ROLE: u8> {
    pub(crate) image: &'static crate::global::compiled::images::CompiledRoleImage,
}

impl<const ROLE: u8> RoleProgram<ROLE> {
    pub(crate) const fn new(
        image: &'static crate::global::compiled::images::CompiledRoleImage,
    ) -> Self {
        Self { image }
    }

    #[inline(always)]
    pub(crate) const fn compiled_role_image(
        &self,
    ) -> &'static crate::global::compiled::images::CompiledRoleImage {
        self.image
    }
}

impl<const ROLE: u8> private::RoleProgramViewSeal for RoleProgram<ROLE> {}

impl<const ROLE: u8> RoleProgramView<ROLE> for RoleProgram<ROLE> {
    #[inline(always)]
    fn compiled_role_image(&self) -> &'static crate::global::compiled::images::CompiledRoleImage {
        RoleProgram::compiled_role_image(self)
    }
}

pub(crate) const fn project_typed_program<const ROLE: u8, Steps>(
    program: &Program<Steps>,
) -> RoleProgram<ROLE>
where
    Steps: BuildProgramSource,
{
    crate::global::validate_role_index(ROLE);
    let _ = program;
    RoleProgram::new(&ValidatedRoleImage::<Steps, ROLE>::COMPILED_IMAGE)
}

/// Project a typed program into the local view for `ROLE`.
pub fn project<const ROLE: u8, Steps>(program: &Program<Steps>) -> RoleProgram<ROLE>
where
    Steps: BuildProgramSource,
{
    project_typed_program(program)
}
