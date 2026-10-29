use super::super::PackedLocalEventRow;
use super::{
    event_rows::RoleLocalDirection,
    projection::{LocalRows, ScopeFacts},
};
use crate::global::const_dsl::EffList;
use crate::global::{
    const_dsl::ScopeId,
    typestate::{LocalConflict, PackedEventConflict, PackedLocalDependency},
};

#[derive(Clone, Copy)]
pub(super) struct ProjectedEvent {
    pub(super) event: PackedLocalEventRow,
    pub(super) dependency: PackedLocalDependency,
    pub(super) conflict: PackedEventConflict,
    pub(super) lane: u8,
}

/// Immutable operation permutations and causal links, computed from source
/// rows before serialization. Two rows cover both facets of a source event.
pub(crate) struct OperationIndex<const E: usize> {
    steps: [[u16; E]; 2],
    predecessors: [[u16; E]; 2],
    view_lanes: [[u16; E]; 2],
    view_keys: [[u32; E]; 2],
    rows: [[Option<ProjectedEvent>; E]; 2],
    pub(super) retained: [bool; ScopeId::LOCAL_CAPACITY as usize],
    pub(super) dependency_count: usize,
    pub(super) conflict_count: usize,
    pub(super) max_lane: usize,
    len: usize,
}

impl<const E: usize> OperationIndex<E> {
    pub(crate) const fn new(source: (&EffList<E>, &ScopeFacts<E>), rows: LocalRows<'_, E>) -> Self {
        let (source, scopes) = source;
        let mut out = Self {
            steps: [[0; E]; 2],
            predecessors: [[u16::MAX; E]; 2],
            view_lanes: [[0; E]; 2],
            view_keys: [[0; E]; 2],
            rows: [[None; E]; 2],
            retained: [false; ScopeId::LOCAL_CAPACITY as usize],
            dependency_count: 0,
            conflict_count: 0,
            max_lane: 0,
            len: 0,
        };
        let mut keys = [[0u32; E]; 2];
        let mut effects = [[0u16; E]; 2];
        let mut last_view = [u16::MAX; 256];
        let topology = scopes.route_count() != 0;
        let local_len = rows.range(source, 0, source.len()).len();
        let mut dependencies = super::projection::DependencyCursor::new(source, scopes, rows);
        let mut event = 0;
        while event < source.len() {
            let atom = source.atom_at(event);
            let mut side = 0;
            while side < rows.count(source, event) {
                let step = out.len;
                if step >= 2 * E || step >= u16::MAX as usize {
                    crate::invariant();
                }
                let direction = rows.direction(source, event, side);
                let (local, conflict) =
                    super::projection::local_event_row_for_eff((source, scopes), event, rows, side);
                let dependency = dependencies.next(event, atom.lane, step);
                out.rows[step / E][step % E] = Some(ProjectedEvent {
                    event: local,
                    dependency,
                    conflict,
                    lane: atom.lane,
                });
                if !dependency.is_none() {
                    out.dependency_count += 1;
                }
                if !conflict.is_none() {
                    out.conflict_count += 1;
                }
                if atom.lane as usize + 1 > out.max_lane {
                    out.max_lane = atom.lane as usize + 1;
                }
                if let Some(dependency) = dependency.to_dependency(local_len)
                    && let LocalConflict::RouteArm { scope, .. } = dependency.conflict()
                {
                    out.retained[scope.local_ordinal() as usize] = true;
                }
                let key = direction.operation_index_key(atom.from, atom.to, atom.label);
                out.steps[step / E][step % E] = step as u16;
                keys[step / E][step % E] = key;
                if topology {
                    let view = if matches!(direction, RoleLocalDirection::Recv) {
                        atom.to
                    } else {
                        atom.from
                    };
                    effects[step / E][step % E] = event as u16;
                    out.view_keys[step / E][step % E] = ((view as u32) << 8) | atom.lane as u32;
                    out.view_lanes[step / E][step % E] = step as u16;
                    let mut prior = last_view[view as usize];
                    while prior != u16::MAX {
                        let before = effects[prior as usize / E][prior as usize % E] as usize;
                        let Some(cut) = scopes.predecessor_cut(source, before, event) else {
                            break;
                        };
                        let mut candidate = rows.range(source, 0, cut).end();
                        prior = u16::MAX;
                        while candidate != 0 {
                            candidate -= 1;
                            if (out.view_keys[candidate / E][candidate % E] >> 8) as u8 == view {
                                prior = candidate as u16;
                                break;
                            }
                        }
                    }
                    out.predecessors[step / E][step % E] = prior;
                    last_view[view as usize] = step as u16;
                }
                out.len += 1;
                side += 1;
            }
            event += 1;
        }
        Self::sort(&mut out.steps, &keys, out.len, 17);
        if topology {
            Self::sort(&mut out.view_lanes, &out.view_keys, out.len, 16);
        }
        out.retained =
            super::projection::finish_route_retention((source, scopes), rows, out.retained);
        out
    }

    /// Stable byte-radix order. The final operation-key pass has one bit.
    const fn sort(steps: &mut [[u16; E]; 2], keys: &[[u32; E]; 2], len: usize, bits: u32) {
        if len < 2 {
            return;
        }
        let mut scratch = [[0u16; E]; 2];
        let mut shift = 0;
        while shift < bits {
            let buckets = if bits - shift < 8 {
                1 << (bits - shift)
            } else {
                256
            };
            let mask = buckets - 1;
            let mut positions = [0usize; 256];
            let mut slot = 0;
            while slot < len {
                let step = steps[slot / E][slot % E] as usize;
                let key = keys[step / E][step % E];
                positions[((key >> shift) as usize) & mask] += 1;
                slot += 1;
            }
            let mut bucket = 0;
            let mut start = 0;
            while bucket < buckets {
                let count = positions[bucket];
                positions[bucket] = start;
                start += count;
                bucket += 1;
            }
            slot = 0;
            while slot < len {
                let step = steps[slot / E][slot % E];
                let key = keys[step as usize / E][step as usize % E];
                let bucket = ((key >> shift) as usize) & mask;
                let target = positions[bucket];
                scratch[target / E][target % E] = step;
                positions[bucket] += 1;
                slot += 1;
            }
            slot = 0;
            while slot < len {
                steps[slot / E][slot % E] = scratch[slot / E][slot % E];
                slot += 1;
            }
            shift += 8;
        }
    }

    pub(crate) const fn len(&self) -> usize {
        self.len
    }
    pub(super) const fn row(&self, step: usize) -> ProjectedEvent {
        if step >= self.len {
            crate::invariant();
        }
        match self.rows[step / E][step % E] {
            Some(row) => row,
            None => crate::invariant(),
        }
    }
    pub(crate) const fn retained(&self) -> &[bool] {
        &self.retained
    }
    pub(crate) const fn at(&self, slot: usize) -> usize {
        if slot >= self.len {
            crate::invariant();
        }
        self.steps[slot / E][slot % E] as usize
    }
    pub(crate) const fn predecessor(&self, step: usize) -> u16 {
        if step >= self.len {
            crate::invariant();
        }
        self.predecessors[step / E][step % E]
    }
    pub(crate) const fn view_lane_at(&self, slot: usize) -> u16 {
        if slot >= self.len {
            crate::invariant();
        }
        self.view_lanes[slot / E][slot % E]
    }
    pub(crate) const fn view_lane_key(&self, step: usize) -> u32 {
        if step >= self.len {
            crate::invariant();
        }
        self.view_keys[step / E][step % E]
    }
}
