"""Check that differential definitions remain the snapshotted original bodies."""
from pathlib import Path
import hashlib, json
here=Path(__file__).resolve().parent
repo=here.parents[1]/'hibana-projection-conflict-reuse'
blob=(here/'baseline-blob_image.rs').read_text()
start=blob.index('    pub(crate) const fn emit<const E: usize>(')
emit=blob[start:blob.rfind('\n}')]
emit=emit.replace('pub(crate) const fn emit<','pub(super) const fn emit_reference<')
emit=emit.replace('projection::local_event_row_for_eff(', 'local_event_row_for_eff(')
emit=emit.replace('super::super::PackedRouteArmRow::new','crate::global::role_program::PackedRouteArmRow::new')
projection=(here/'baseline-projection.rs').read_text()
helper=projection[projection.index('const fn route_scope_and_arm_at'):]
helper=helper.replace('route_conflict_for_eff(', 'projection::route_conflict_for_eff(')
helper=helper.replace('scope_at(eff_list', 'projection::scope_at(eff_list')
helper=helper.replace('route_arm_ranges(', 'projection::route_arm_ranges(')
helper=helper.replace('binary_route_arm_index(', 'super::super::super::binary_route_arm_index(')
expected='// Frozen pre-edit emitter and row helpers; only names/visibility/import paths adapted.\nuse super::super::*;\nuse crate::global::const_dsl::ScopeId;\nuse crate::global::typestate::{LocalConflict, RouteChoiceMark};\n\n'+helper+'\nimpl<const N: usize> RoleImageBytes<N> {\n'+emit+'\n}\n'
path=repo/'src/global/role_program/image_impl/tests/conflict_reuse/original.rs'
assert path.read_text()==expected
record=json.loads((here/'pre-edit-gate.json').read_text())
for name in ['blob_image.rs','projection.rs']:
    assert hashlib.sha256((here/('baseline-'+name)).read_bytes()).hexdigest()==record['source_sha256_before']['src/global/role_program/image_impl/'+name]
print('PASS frozen emitter and all three original row helper bodies; baseline hashes match pre-edit gate')
