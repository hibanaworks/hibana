//! Construction of immutable source-order rows and event-to-scope links.
use super::{
    super::columns::{PROGRAM_IMAGE_SCOPE_MARKER_STRIDE, ProgramImageColumns},
    CompiledProgramRef,
};
use crate::global::const_dsl::{ScopeId, ScopeKind};

pub(crate) struct SourceOrderIndex<const E: usize> {
    ranges: [[u16; 3]; E],
    parents: [u16; E],
    heads: [u16; E],
    roll_heads: [u16; E],
    kinds: [u8; E],
    events: usize,
    scopes: usize,
    rolls: bool,
}

impl<const E: usize> SourceOrderIndex<E> {
    pub(crate) const fn from_program(program: &CompiledProgramRef) -> Option<Self> {
        Self::from_bytes(
            program.columns,
            program.source_scope_marker_count(),
            program.blob.as_slice(),
        )
    }

    pub(crate) const fn layout(
        columns: ProgramImageColumns,
        markers: usize,
        bytes: &[u8],
    ) -> Option<(usize, bool)> {
        let column = columns.scope_markers_for(markers);
        if column.end_offset(PROGRAM_IMAGE_SCOPE_MARKER_STRIDE) > bytes.len() {
            return None;
        }
        let mut extent = 0;
        let mut rolls = false;
        let mut marker = 0;
        while marker < markers {
            let at = column.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
            let Some(scope) = ScopeId::decode_raw(read_u16(bytes, at + 2)) else {
                return None;
            };
            if scope.kind().is_some() {
                if matches!(scope.kind(), Some(ScopeKind::Roll)) {
                    rolls = true;
                }
                let end = scope.local_ordinal() as usize + 1;
                if end > extent {
                    extent = end;
                }
            }
            marker += 1;
        }
        Some((extent, rolls))
    }

    pub(crate) const fn from_bytes(
        columns: ProgramImageColumns,
        markers: usize,
        bytes: &[u8],
    ) -> Option<Self> {
        let mut index = Self {
            ranges: [[u16::MAX; 3]; E],
            parents: [u16::MAX; E],
            heads: [u16::MAX; E],
            roll_heads: [u16::MAX; E],
            kinds: [u8::MAX; E],
            events: columns.atom_count(),
            scopes: 0,
            rolls: false,
        };
        if index.events > E {
            return None;
        }
        let markers = columns.scope_markers_for(markers);
        if markers.end_offset(PROGRAM_IMAGE_SCOPE_MARKER_STRIDE) > bytes.len() {
            return None;
        }
        let mut count = 0;
        let mut marker = 0;
        while marker < markers.len as usize {
            let at = markers.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
            let raw_scope = read_u16(bytes, at + 2);
            let Some(scope) = ScopeId::decode_raw(raw_scope) else {
                return None;
            };
            if let Some(scope_kind) = scope.kind() {
                let slot = scope.local_ordinal() as usize;
                if slot >= E {
                    return None;
                }
                if slot >= count {
                    count = slot + 1;
                }
                let kind = scope_kind as u8;
                if matches!(scope_kind, ScopeKind::Roll) {
                    index.rolls = true;
                }
                if index.kinds[slot] != u8::MAX && index.kinds[slot] != kind {
                    return None;
                }
                index.kinds[slot] = kind;
                let point = read_u16(bytes, at);
                let row = &mut index.ranges[slot];
                match bytes[at + 4] & 3 {
                    0 if row[0] == u16::MAX => row[0] = point,
                    2 if matches!(scope_kind, ScopeKind::Roll) && row[2] == u16::MAX => {
                        row[2] = point
                    }
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
        // Validate every interval before a single marker/event walk links its
        // parent and innermost enclosing scope. IDs may be sparse or unsorted.
        // Keep separate links for repetitions and binary scopes. Each runtime
        // query follows only the kind it needs, without filtering other scopes.
        if count == 0 {
            return Some(index);
        }
        let mut scope = 0;
        let mut present = 0;
        while scope < count {
            let [start, split, end] = index.ranges[scope];
            if start != u16::MAX {
                present += 1;
                if index.kinds[scope] == ScopeKind::Roll as u8 {
                    if start >= end || end as usize > index.events || split != u16::MAX {
                        return None;
                    }
                } else if start >= split || split >= end || end as usize > index.events {
                    return None;
                }
            }
            scope += 1;
        }
        let mut marker = 0;
        let mut opened = 0;
        let mut current = u16::MAX;
        let mut current_roll = u16::MAX;
        let mut stack = [u16::MAX; E];
        let mut depth = 0;
        let mut event = 0;
        while event < index.events {
            while depth != 0 {
                let slot = stack[depth - 1] as usize;
                if event < index.ranges[slot][2] as usize {
                    break;
                }
                if index.kinds[slot] == ScopeKind::Roll as u8 {
                    current_roll = index.parents[slot];
                } else {
                    current = index.parents[slot];
                }
                depth -= 1;
            }
            while marker < markers.len as usize {
                let at = markers.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
                let point = read_u16(bytes, at) as usize;
                if point > event {
                    break;
                }
                let Some(scope) = ScopeId::decode_raw(read_u16(bytes, at + 2)) else {
                    return None;
                };
                let slot = scope.local_ordinal() as usize;
                if scope.kind().is_some()
                    && bytes[at + 4] & 3 == 0
                    && index.ranges[slot][0] as usize == point
                {
                    if point != event
                        || (depth != 0
                            && index.ranges[stack[depth - 1] as usize][2] < index.ranges[slot][2])
                    {
                        return None;
                    }
                    if index.kinds[slot] == ScopeKind::Roll as u8 {
                        index.parents[slot] = current_roll;
                        current_roll = slot as u16;
                    } else {
                        index.parents[slot] = current;
                        current = slot as u16;
                    }
                    stack[depth] = slot as u16;
                    depth += 1;
                    opened += 1;
                }
                marker += 1;
            }
            index.heads[event] = current;
            index.roll_heads[event] = current_roll;
            event += 1;
        }
        if opened != present {
            return None;
        }
        index.scopes = count;
        Some(index)
    }

    pub(crate) const fn packed_len(&self) -> usize {
        if self.scopes == 0 {
            0
        } else {
            self.scopes * 9 + self.events * if self.rolls { 4 } else { 2 }
        }
    }

    pub(crate) const fn packed_byte(&self, offset: usize) -> u8 {
        if offset < self.scopes * 9 {
            let row = offset / 9;
            let field = offset % 9;
            if field == 8 {
                return self.kinds[row];
            }
            let value = if field < 6 {
                self.ranges[row][field / 2]
            } else {
                self.parents[row]
            };
            (value >> (8 * (field % 2))) as u8
        } else {
            let offset = offset - self.scopes * 9;
            let value = if offset < self.events * 2 {
                self.heads[offset / 2]
            } else {
                self.roll_heads[(offset - self.events * 2) / 2]
            };
            (value >> (8 * (offset % 2))) as u8
        }
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

const fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
