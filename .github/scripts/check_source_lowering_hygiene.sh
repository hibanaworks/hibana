#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

check_absent_multiline \
  'MAX_EFF_NODES|MAX_SEGMENTS|MAX_SEGMENT_EFFS|ProgramTerm|struct[[:space:]]+ProgramSourceData[[:space:]]*\{' \
  "source lowering must not retain the old fixed segmented capacity or source trait" \
  src

check_required_regex 'COMPACT_EVENT_IDENTITY_CAPACITY:\s*usize\s*=\s*u16::MAX\s+as\s+usize' \
  "dense event identity must derive from the compact u16 descriptor domain" \
  src/eff.rs
check_required_regex 'pub\(crate\)\s+const\s+fn\s+from_dense_ordinal' \
  "dense EffIndex construction must remain crate-private and honestly named" \
  src/eff.rs
check_required_regex 'pub\(crate\)\s+const\s+fn\s+dense_ordinal\(self\)\s*->\s*usize' \
  "dense EffIndex access must remain crate-private and honestly named" \
  src/eff.rs
check_absent 'pub[[:space:]]+const[[:space:]]+fn[[:space:]]+(from_dense_ordinal|dense_ordinal|raw)|from_segment_offset|fn[[:space:]]+segment\(|fn[[:space:]]+offset\(' \
  "EffIndex representation must not enter the public API or regain segmented aliases" \
  src/eff.rs

check_required 'pub(crate) trait ProgramShape {' \
  "one-pass source lowering requires one crate-private choreography shape owner" \
  src/g/source.rs
for required in \
  'const SOURCE_NODE: ProgramSourceNode;' \
  'const EVENT_COUNT: usize;' \
  'const SCOPE_MARKER_COUNT: usize;' \
  'const RESOLVER_MARKER_COUNT: usize;' \
  'const SOURCE_ROW_COUNT: usize = checked_source_count(' \
  'struct SourceLowering<const CAPACITY: usize>' \
  'const fn emit(&mut self, node: &ProgramSourceNode, route_reentry: ReentryMark) -> u16' \
  'let mut lowering = SourceLowering::new(' \
  'let _ = lowering.emit(&Steps::SOURCE_NODE, ReentryMark::SinglePass);' \
  'if lowering.eff.len() != Steps::EVENT_COUNT' \
  'merge_parallel_lanes(' \
  'merge_route_frame_labels(' \
  'color_roll_frame_labels(&mut self.eff, start, end);'
do
  check_required "${required}" \
    "one-pass source lowering contract missing: ${required}" \
    src/g/source.rs
done
check_required 'struct EndpointSet<const ROLE_BYTES: usize>' \
  "parallel lane proof representation must erase to the production role domain" \
  src/global/const_dsl/allocation/lane_matching/index.rs
check_required 'struct LaneEndpointIndex<const ROLE_BYTES: usize>' \
  "parallel lane coloring must derive endpoint slots from the wire lane domain" \
  src/global/const_dsl/allocation/lane_matching/index.rs
check_required 'slot_by_lane: [u16; BYTE_DOMAIN]' \
  "parallel lane lookup must derive exactly from the wire lane domain" \
  src/global/const_dsl/allocation/lane_matching/index.rs
check_required 'endpoints: [EndpointSet<ROLE_BYTES>; BYTE_DOMAIN]' \
  "parallel endpoint-role sets must stay bounded by the wire lane domain" \
  src/global/const_dsl/allocation/lane_matching/index.rs
check_absent 'lane_role_masks|lane_endpoint_sets|endpoints:[[:space:]]*\[EndpointSet<ROLE_BYTES>;[[:space:]]*E\]' \
  "parallel lane coloring must not scale matching scratch with source events" \
  src/global/const_dsl/allocation/lane_matching.rs \
  src/global/const_dsl/allocation/lane_matching/index.rs

check_required 'pub(crate) struct EffList<const ARENA_CAPACITY: usize>' \
  "EffList must keep one exact-capacity tagged source arena" \
  src/global/const_dsl/source_arena.rs
for required in \
  'enum SourceRow {' \
  'Event { atom: EffAtom, frame_label: u8 }' \
  'Scope(ScopeMarker)' \
  'Resolver(RouteResolverMarker)' \
  'rows: [SourceRow; ARENA_CAPACITY]' \
  'EffList::new_partitioned(event_count, scope_count, resolver_count)'
do
  check_required "${required}" \
    "single source arena contract missing: ${required}" \
    src/global/const_dsl/source_arena.rs src/g/source.rs
done
check_absent 'events:[[:space:]]*\[EffAtom|frame_labels:[[:space:]]*\[u8|scope_markers:[[:space:]]*\[ScopeMarker|resolver_markers:[[:space:]]*\[RouteResolverMarker|from_raw_parts' \
  "source lowering must not restore parallel maximum-capacity arrays or raw prefix slices" \
  src/global/const_dsl.rs src/global/const_dsl/source_arena.rs src/global/const_dsl/eff_list.rs
check_absent 'EffList<[^>]*,|EffList::<[^>]*,|const SCOPE_MARKER_CAPACITY: usize|const RESOLVER_CAPACITY: usize' \
  "single source arena must not retain redundant marker-capacity const generics" \
  src/g.rs src/g/source.rs src/global/const_dsl src/global/compiled src/global/role_program
check_absent 'event_capacity: usize|resolver_capacity: usize|partition exceeds capacity' \
  "single source arena boundaries must be derived without duplicate capacity fields" \
  src/global/const_dsl/source_arena.rs src/global/const_dsl/eff_list.rs
check_required 'if required > E' \
  "source arena partition must fit its stable-Rust dispatch bucket" \
  src/global/const_dsl/eff_list.rs
check_required 'source_end: usize' \
  "source arena must retain one exact terminal partition boundary" \
  src/global/const_dsl/source_arena.rs
for required in \
  'pub(crate) const fn covers_source_counts(' \
  'const SOURCE_COUNTS_COVERED: ()' \
  'let () = Self::SOURCE_COUNTS_COVERED;'
do
  check_required "${required}" \
    "final program bytes must cover every exact source row: ${required}" \
    src/global/compiled/images/image/columns.rs src/g/role_projection.rs
done
check_absent '#\[derive\([^]]*(Clone|Copy)[^]]*\)\][[:space:]]*pub\(crate\) struct EffList|struct SegmentSummary|segment_summaries:|segment_summary\(|segment_count\(|segment_len\(' \
  "source metadata must not be copied or retain segmented summary residue" \
  src/global/const_dsl.rs src/global/const_dsl/source_arena.rs src/global/const_dsl/eff_list.rs
for required in \
  'pub(crate) const fn first_enter_index(self, scope: ScopeId)' \
  'self.at(index).event.is_primary_enter()'
do
  check_required "${required}" \
    "scope-marker first-enter authority missing: ${required}" \
    src/global/const_dsl/source_arena.rs
done
check_required 'pub(crate) const fn route_arm_event_ranges_for_scope(' \
  "route-arm scope lookup must have one canonical owner" \
  src/global/const_dsl/scope_ranges/route.rs
check_required 'pub(crate) const fn scope_segment_end_from_enter(' \
  "closed scope-segment bounds must have one canonical owner" \
  src/global/const_dsl/scope_ranges.rs
for required in \
  'pub(crate) const fn structured_scope_event_range(' \
  'pub(crate) const fn route_scope_slot_for_scope(' \
  'pub(crate) const fn route_parent_arm_for_scope(' \
  'pub(crate) const fn passive_route_child_scope('
do
  check_required "${required}" \
    "scope topology must have one canonical owner: ${required}" \
    src/global/const_dsl/scope_ranges/route.rs
done
check_absent 'first_enter_for_scope|scope_first_enter_index|scope_full_end|scope_first_enter_offset|passive_child_route_(scope|enter_index)|nearest_route_parent_for_scope|const[[:space:]]+fn[[:space:]]+(route_arm_ranges|scope_segment_end|route_scope_slot_for_scope)\(' \
  "scope-marker and route-arm lookup must not regain duplicate authorities" \
  src/global/compiled src/global/role_program

for required in \
  'if required_rows <= 8' \
  'else if required_rows <= 32' \
  'else if required_rows <= 128' \
  'else if required_rows <= 512' \
  'else if required_rows <= 2048' \
  'else if required_rows <= 8192' \
  'else if required_rows <= 32768' \
  'else if required_rows <= 65535' \
  'panic!("choreography source exceeds compact descriptor domain")'
do
  check_required "${required}" \
    "source bucket dispatch must cover the compact descriptor domain: ${required}" \
    src/g.rs
done
check_absent '\b(3072|96[[:space:]]*\*[[:space:]]*32)\b|source bucket.*fallback|unwrap_or|unwrap_or_else' \
  "source lowering must not retain the old fixed ceiling or fallback bucket" \
  src/eff.rs src/g.rs src/g/source.rs src/g/role_projection.rs src/global/const_dsl.rs src/global/const_dsl

for required in \
  'augment_lane_matching' \
  'right_stack = [0u16; BYTE_DOMAIN]' \
  'right_to_left = [NO_MATCH; BYTE_DOMAIN]' \
  'Maximum bipartite matching reuses the greatest' \
  'merge_parallel_lanes' \
  'parallel endpoint lane coloring exceeds wire domain'
do
  check_required "${required}" \
    "wire-domain coloring implementation missing: ${required}" \
    src/global/const_dsl/allocation/lane_matching.rs
done
check_required 'const BYTE_DOMAIN: usize = u8::MAX as usize + 1;' \
  "lane and frame colors must derive from the complete wire byte domain" \
  src/global/const_dsl/allocation.rs
check_absent 'MATCHING_STACK_CAPACITY|conflicting_left_lanes|candidate_taken|first_fit_lane|first_fit_remap|right_stack = \[0u16; E\]|right_to_left = \[NO_MATCH; E\]' \
  "parallel lane allocation must stay wire-domain bounded and not regress to source-order first-fit" \
  src/global/const_dsl/allocation/lane_matching.rs
check_required 'route inbound occurrence coloring exceeds wire domain' \
  "route frame coloring must fail closed at the wire domain" \
  src/global/const_dsl/allocation/frame_labels/route.rs
check_required 'roll inbound occurrence coloring exceeds wire domain' \
  "roll frame coloring must fail closed at the wire domain" \
  src/global/const_dsl/allocation/frame_labels/roll.rs
for required in \
  'pub(crate) use roll::color_roll_frame_labels;' \
  'pub(crate) use route::merge_route_frame_labels;'
do
  check_required "${required}" \
    "frame coloring must retain one canonical owner: ${required}" \
    src/global/const_dsl/allocation/frame_labels.rs
done

check_absent 'struct[[:space:]]+ProgramImageSegmentData|struct[[:space:]]+ProgramImageValidationData|struct[[:space:]]+ProgramAtomRow|CompiledProgramView|atom_rows:|route_resolver_sites:' \
  "compiled lowering must not rebuild source atom or resolver authority" \
  src/global/compiled/lowering/driver.rs src/global/compiled/lowering/driver
check_required_regex 'atom_at\(|resolver_for_scope\(' \
  "compact lowering must consume EffList as its single source" \
  src/global/const_dsl/eff_list.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "source lowering hygiene check passed"
