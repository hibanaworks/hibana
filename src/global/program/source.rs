#[cfg(test)]
#[inline(always)]
pub(crate) const fn boundary_source_program_image(
    eff_list: &crate::global::const_dsl::EffList,
) -> crate::global::compiled::lowering::CompiledProgramImage {
    crate::global::compiled::lowering::CompiledProgramImage::scan_const(eff_list)
}
