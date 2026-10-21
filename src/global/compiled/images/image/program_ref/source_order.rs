//! Compiler scratch for checking all local pairs against the source scopes.
use super::{super::columns::PROGRAM_IMAGE_SCOPE_MARKER_STRIDE, CompiledProgramRef};
use crate::global::const_dsl::{ScopeId, ScopeKind};

pub(crate) struct SourceOrderIndex<const E: usize> {
    ranges: [[u16; 3]; E],
    parents: [u16; E],
    heads: [u16; E],
    events: usize,
}

impl<const E: usize> SourceOrderIndex<E> {
    pub(crate) const fn from_program(program: &CompiledProgramRef) -> Option<Self> {
        let mut index = Self {
            ranges: [[u16::MAX; 3]; E],
            parents: [u16::MAX; E],
            heads: [u16::MAX; E],
            events: program.columns.atom_count(),
        };
        if index.events > E {
            return None;
        }
        let markers = program
            .columns
            .scope_markers_for(program.source_scope_marker_count());
        let mut count = 0;
        let mut marker = 0;
        while marker < markers.len as usize {
            let at = markers.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
            let raw_scope = program.read_u16_at(at + 2);
            let Some(scope) = ScopeId::decode_raw(raw_scope) else {
                return None;
            };
            if matches!(scope.kind(), Some(ScopeKind::Parallel | ScopeKind::Route)) {
                let slot = scope.local_ordinal() as usize;
                if slot >= E {
                    return None;
                }
                if slot >= count {
                    count = slot + 1;
                }
                let point = program.read_u16_at(at);
                let row = &mut index.ranges[slot];
                match program.byte_at(at + 4) & 3 {
                    0 if row[0] == u16::MAX => row[0] = point,
                    0 if matches!(scope.kind(), Some(ScopeKind::Route)) && row[1] == point => (),
                    1 if matches!(scope.kind(), Some(ScopeKind::Parallel)) => row[1] = point,
                    2 if matches!(scope.kind(), Some(ScopeKind::Route)) && row[1] == u16::MAX => {
                        row[1] = point;
                    }
                    2 => row[2] = point,
                    _ => return None,
                }
            }
            marker += 1;
        }
        // Scope ordinals are preorder positions. Each nested scope replaces
        // its parent's event heads, retaining that parent as its only link.
        let mut scope = 0;
        while scope < count {
            let [start, split, end] = index.ranges[scope];
            if start != u16::MAX {
                if start >= split || split >= end || end as usize > index.events {
                    return None;
                }
                let parent = index.heads[start as usize];
                index.parents[scope] = parent;
                let mut event = start as usize;
                while event < end as usize {
                    if index.heads[event] != parent {
                        return None;
                    }
                    index.heads[event] = scope as u16;
                    event += 1;
                }
            }
            scope += 1;
        }
        Some(index)
    }

    pub(crate) const fn before(&self, first: usize, second: usize) -> bool {
        if first >= second || second >= self.events {
            return false;
        }
        let mut scope = self.heads[first];
        while scope != u16::MAX {
            let [_, split, end] = self.ranges[scope as usize];
            if first < split as usize && split as usize <= second && second < end as usize {
                return false;
            }
            scope = self.parents[scope as usize];
        }
        true
    }
}
