use super::super::CompiledProgramRef;
use crate::global::compiled::images::image::columns::{OwnershipColumns, ProgramImageColumns};
use crate::{
    Endpoint, g,
    global::compiled::{
        images::{ProgramImageBytes, ProgramImagePlan},
        lowering::CompiledProgramImage,
    },
};

type Grant = g::Msg<1, Endpoint<'static, 255>>;
type Work = g::Msg<2, ()>;
type Steps = g::Seq<
    g::Resolve<g::Route<g::Send<0, 1, Grant>, g::Send<0, 2, Grant>>, 701>,
    g::Send<255, 3, Work>,
>;
pub(super) const SOURCE: g::ProgramSourceData<32> = g::ProgramSourceData::lower::<Steps>();
const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
pub(super) const COLUMNS: ProgramImageColumns = PLAN.columns();
pub(super) const SUMMARY: CompiledProgramImage =
    CompiledProgramImage::scan_const(SOURCE.eff_list());
pub(super) const OWNERSHIP: OwnershipColumns =
    match COLUMNS.ownership_columns(SOURCE.eff_list().scope_marker_count(), 1) {
        Some(columns) => columns,
        None => panic!("owned fixture columns"),
    };
static BYTES: ProgramImageBytes<128> =
    match ProgramImageBytes::from_source_if_fits(&SOURCE, COLUMNS) {
        Some(bytes) => bytes,
        None => panic!("owned fixture storage"),
    };
pub(super) const PROGRAM: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, COLUMNS);

pub(super) const fn encoded_bytes() -> [u8; 128] {
    let mut bytes = [0; 128];
    let mut index = 0;
    while index < COLUMNS.blob_len() {
        bytes[index] = PROGRAM.byte_at(index);
        index += 1;
    }
    let mut offset = 0;
    while offset < PROGRAM.source_order.len() {
        bytes[COLUMNS.blob_len() + offset] = PROGRAM.source_order.byte_at(offset);
        offset += 1;
    }
    bytes
}
