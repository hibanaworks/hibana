use super::{
    CompiledProgramImage, Program, ProgramSourceData, ProgramStamp, RoleFacts, RoleImage,
    RoleImageRef, RoleImageSource, RoleLaneImage, RoleProgramView, private,
    validated_program_image,
};
struct ValidatedRoleImage<Steps, const ROLE: u8>(core::marker::PhantomData<Steps>);

impl<Steps, const ROLE: u8> ValidatedRoleImage<Steps, ROLE>
where
    Steps: crate::g::ChoreographyTerm<Source = ProgramSourceData>,
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
    Steps: crate::g::ChoreographyTerm<Source = ProgramSourceData>,
{
    crate::global::validate_role_index(ROLE);
    let _ = program;
    RoleProgram::new(&ValidatedRoleImage::<Steps, ROLE>::COMPILED_IMAGE)
}

mod projectable_program_seal {
    pub trait Sealed {}

    impl<Steps> Sealed for super::Program<Steps> where
        Steps: crate::g::ChoreographyTerm<Source = super::ProgramSourceData>
    {
    }
}

/// Canonical projection input accepted by `integration::program::project`.
pub trait ProjectableProgram: projectable_program_seal::Sealed {
    /// Project this typed choreography into the local view for `ROLE`.
    fn project_role<const ROLE: u8>(&self) -> RoleProgram<ROLE>;
}

impl<Steps> ProjectableProgram for Program<Steps>
where
    Steps: crate::g::ChoreographyTerm<Source = ProgramSourceData>,
{
    #[inline]
    fn project_role<const ROLE: u8>(&self) -> RoleProgram<ROLE> {
        project_typed_program(self)
    }
}

/// Project a typed program into the local view for `ROLE`.
pub fn project<const ROLE: u8, P>(program: &P) -> RoleProgram<ROLE>
where
    P: ProjectableProgram + ?Sized,
{
    program.project_role()
}
