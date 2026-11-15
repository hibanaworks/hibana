use crate::global::{
    compiled::lowering::CompiledProgramImage,
    const_dsl::{EffList, ScopeId, ScopeKind, route_arm_ranges_from_first_enter},
};

pub(crate) const PROGRAM_IMAGE_ATOM_STRIDE: usize = 9;
pub(crate) const PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 8;
pub(crate) const PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE: usize = 1;
pub(crate) const PROGRAM_IMAGE_SCOPE_MARKER_STRIDE: usize = 5;
pub(super) const PROGRAM_IMAGE_ACTOR_BINDING_STRIDE: usize = 4;
pub(super) const PROGRAM_IMAGE_ACTOR_DECISION_STRIDE: usize = 6;
pub(crate) const ACTOR_DECISION_CAPACITY: usize =
    u16::MAX as usize / PROGRAM_IMAGE_ACTOR_DECISION_STRIDE;
pub(super) const PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE: usize = 2;
pub(crate) const COMPACT_DESCRIPTOR_BYTE_CAPACITY: usize = u16::MAX as usize;
pub(crate) const PROGRAM_IMAGE_MIN_SCOPE_BYTES: usize = 2 * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
const PROGRAM_IMAGE_SCOPE_CAPACITY: usize =
    COMPACT_DESCRIPTOR_BYTE_CAPACITY / PROGRAM_IMAGE_MIN_SCOPE_BYTES;
pub(crate) const PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY: usize =
    COMPACT_DESCRIPTOR_BYTE_CAPACITY / PROGRAM_IMAGE_ATOM_STRIDE;
pub(crate) const PROGRAM_IMAGE_STORAGE_BYTE_CAPACITY: usize = COMPACT_DESCRIPTOR_BYTE_CAPACITY
    + ScopeId::LOCAL_CAPACITY as usize * 9
    + PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY * 4
    + ScopeId::LOCAL_CAPACITY as usize * 2;
const _: () = assert!(
    PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY < crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY
);
const _: () = assert!(PROGRAM_IMAGE_SCOPE_CAPACITY < ScopeId::LOCAL_CAPACITY as usize);
const _: () = assert!(PROGRAM_IMAGE_SCOPE_CAPACITY < u16::MAX as usize);
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramColumnRange {
    pub(crate) offset: u16,
    pub(crate) len: u16,
}

impl ProgramColumnRange {
    #[inline(always)]
    pub(crate) const fn new(offset: usize, len: usize, stride: usize) -> Self {
        if offset > COMPACT_DESCRIPTOR_BYTE_CAPACITY || len > COMPACT_DESCRIPTOR_BYTE_CAPACITY {
            crate::invariant();
        }
        if stride == 0 {
            crate::invariant();
        }
        let byte_len = match len.checked_mul(stride) {
            Some(byte_len) => byte_len,
            None => crate::invariant(),
        };
        if byte_len > (COMPACT_DESCRIPTOR_BYTE_CAPACITY - offset) {
            crate::invariant();
        }
        Self {
            offset: offset as u16,
            len: len as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn byte_len(self, stride: usize) -> usize {
        self.len as usize * stride
    }

    #[inline(always)]
    pub(crate) const fn end_offset(self, stride: usize) -> usize {
        self.offset as usize + self.byte_len(stride)
    }
}

/// Canonical contiguous program-image counts. Column offsets are derived so an
/// alternate layout cannot become a second identity for the same rows.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramImageColumns {
    atom_len: u16,
    route_resolver_len: u16,
    route_participant_len: u16,
    // Ordinary scope count, or total bytes for the tagged owned grammar.
    tail: u16,
}

pub(super) struct OwnershipColumns {
    pub(super) bindings: ProgramColumnRange,
    pub(super) decisions: ProgramColumnRange,
    pub(super) initial: ProgramColumnRange,
}

impl ProgramImageColumns {
    /// Little-endian encoding of the four actual resident column values.
    #[cfg(all(test, any(hibana_repo_tests, hibana_generated_protocols)))]
    pub(crate) const fn proof_header_bytes(self) -> [u8; 8] {
        let atoms = self.atom_len.to_le_bytes();
        let resolvers = self.route_resolver_len.to_le_bytes();
        let participants = self.route_participant_len.to_le_bytes();
        let tail = self.tail.to_le_bytes();
        [
            atoms[0],
            atoms[1],
            resolvers[0],
            resolvers[1],
            participants[0],
            participants[1],
            tail[0],
            tail[1],
        ]
    }

    // Atom counts fit below 8192. This tag selects the owned image grammar;
    // its final word is the total length and its four-byte tail header stores
    // scope/decision counts. Ordinary column encodings stay unchanged.
    const OWNED: u16 = 1 << 15;
    pub(crate) const fn try_new(
        atom_len: usize,
        route_resolver_len: usize,
        route_participant_len: usize,
        scope_marker_len: usize,
    ) -> Option<Self> {
        if atom_len > PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY
            || route_resolver_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY
            || route_participant_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY
            || scope_marker_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY
        {
            return None;
        }
        let atom_bytes = match atom_len.checked_mul(PROGRAM_IMAGE_ATOM_STRIDE) {
            Some(bytes) => bytes,
            None => return None,
        };
        let resolver_bytes =
            match route_resolver_len.checked_mul(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE) {
                Some(bytes) => bytes,
                None => return None,
            };
        let participant_bytes =
            match route_participant_len.checked_mul(PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE) {
                Some(bytes) => bytes,
                None => return None,
            };
        let marker_bytes = match scope_marker_len.checked_mul(PROGRAM_IMAGE_SCOPE_MARKER_STRIDE) {
            Some(bytes) => bytes,
            None => return None,
        };
        let blob_len = match atom_bytes.checked_add(resolver_bytes) {
            Some(len) => len,
            None => return None,
        };
        let blob_len = match blob_len.checked_add(participant_bytes) {
            Some(len) => len,
            None => return None,
        };
        let blob_len = match blob_len.checked_add(marker_bytes) {
            Some(len) => len,
            None => return None,
        };
        if blob_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY {
            return None;
        }
        Some(Self {
            atom_len: atom_len as u16,
            route_resolver_len: route_resolver_len as u16,
            route_participant_len: route_participant_len as u16,
            tail: scope_marker_len as u16,
        })
    }

    const fn with_actors(self, decisions: usize, initial_actors: usize) -> Option<Self> {
        if self.has_ownership()
            || self.atom_count() == 0
            || decisions > u16::MAX as usize - 256
            || initial_actors == 0
            || initial_actors > 256
        {
            return None;
        }
        let bindings = self.atom_count() * PROGRAM_IMAGE_ACTOR_BINDING_STRIDE;
        let decisions_bytes = decisions * PROGRAM_IMAGE_ACTOR_DECISION_STRIDE;
        let initial_bytes = initial_actors * PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE;
        let total = self.blob_len() + 4 + bindings + decisions_bytes + initial_bytes;
        if total > COMPACT_DESCRIPTOR_BYTE_CAPACITY {
            return None;
        }
        Some(Self {
            atom_len: self.atom_len | Self::OWNED,
            tail: total as u16,
            ..self
        })
    }

    pub(crate) const fn new(
        atom_len: usize,
        route_resolver_len: usize,
        route_participant_len: usize,
        scope_marker_len: usize,
    ) -> Self {
        match Self::try_new(
            atom_len,
            route_resolver_len,
            route_participant_len,
            scope_marker_len,
        ) {
            Some(columns) => columns,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(crate) const fn atoms(self) -> ProgramColumnRange {
        ProgramColumnRange::new(0, self.atom_count(), PROGRAM_IMAGE_ATOM_STRIDE)
    }

    #[inline(always)]
    pub(crate) const fn route_resolvers(self) -> ProgramColumnRange {
        ProgramColumnRange::new(
            self.atom_count() * PROGRAM_IMAGE_ATOM_STRIDE,
            self.route_resolver_len as usize,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        )
    }

    #[inline(always)]
    pub(crate) const fn scope_markers(self) -> ProgramColumnRange {
        if self.has_ownership() {
            crate::invariant();
        }
        self.scope_markers_for(self.tail as usize)
    }

    pub(super) const fn scope_markers_for(self, count: usize) -> ProgramColumnRange {
        ProgramColumnRange::new(
            self.scope_marker_offset() + if self.has_ownership() { 4 } else { 0 },
            count,
            PROGRAM_IMAGE_SCOPE_MARKER_STRIDE,
        )
    }

    #[inline(always)]
    pub(crate) const fn route_participants(self) -> ProgramColumnRange {
        ProgramColumnRange::new(
            self.atom_count() * PROGRAM_IMAGE_ATOM_STRIDE
                + self.route_resolver_len as usize * PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
            self.route_participant_len as usize,
            PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE,
        )
    }

    pub(super) const fn has_ownership(self) -> bool {
        self.atom_len & Self::OWNED != 0
    }

    const fn scope_marker_offset(self) -> usize {
        self.atom_count() * PROGRAM_IMAGE_ATOM_STRIDE
            + self.route_resolver_len as usize * PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE
            + self.route_participant_len as usize * PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE
    }

    pub(super) const fn ownership_header(self) -> ProgramColumnRange {
        if !self.has_ownership() {
            crate::invariant();
        }
        ProgramColumnRange::new(self.scope_marker_offset(), 2, 2)
    }

    pub(super) const fn ownership_columns(
        self,
        scopes: usize,
        decisions: usize,
    ) -> Option<OwnershipColumns> {
        if !self.has_ownership()
            || self.atom_count() == 0
            || scopes > u16::MAX as usize
            || decisions > ACTOR_DECISION_CAPACITY
        {
            return None;
        }
        let binding_start =
            self.ownership_header().end_offset(2) + scopes * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
        let decision_start = binding_start + self.atom_count() * PROGRAM_IMAGE_ACTOR_BINDING_STRIDE;
        let initial_start = decision_start + decisions * PROGRAM_IMAGE_ACTOR_DECISION_STRIDE;
        if initial_start >= self.blob_len() {
            return None;
        }
        let remaining = self.blob_len() - initial_start;
        if !remaining.is_multiple_of(PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE)
            || remaining / PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE > 256
        {
            return None;
        }
        Some(OwnershipColumns {
            bindings: ProgramColumnRange::new(
                binding_start,
                self.atom_count(),
                PROGRAM_IMAGE_ACTOR_BINDING_STRIDE,
            ),
            decisions: ProgramColumnRange::new(
                decision_start,
                decisions,
                PROGRAM_IMAGE_ACTOR_DECISION_STRIDE,
            ),
            initial: ProgramColumnRange::new(
                initial_start,
                remaining / PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE,
                PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE,
            ),
        })
    }

    #[inline(always)]
    pub(crate) const fn atom_count(self) -> usize {
        (self.atom_len & !Self::OWNED) as usize
    }

    #[inline(always)]
    pub(crate) const fn route_resolver_count(self) -> usize {
        self.route_resolver_len as usize
    }

    #[inline(always)]
    pub(crate) const fn scope_marker_count(self) -> usize {
        if self.has_ownership() {
            crate::invariant();
        }
        self.tail as usize
    }

    #[inline(always)]
    pub(crate) const fn route_participant_count(self) -> usize {
        self.route_participant_len as usize
    }

    /// Confirm that source-arena counts fit the exact final columns and byte
    /// length. Descriptor translation validation separately owns row content.
    /// Dynamic resolver markers are a subset of route-resolver rows because
    /// intrinsic routes own a row without a marker.
    #[inline(always)]
    pub(crate) const fn covers_source_counts(
        self,
        event_count: usize,
        scope_marker_count: usize,
        resolver_marker_count: usize,
    ) -> bool {
        // An owned image's exact scope count lives in its validated header.
        // A count-only prefix cannot certify that translation.
        if self.has_ownership() {
            return false;
        }
        let Some(source_rows) = event_count.checked_add(scope_marker_count) else {
            return false;
        };
        let Some(source_rows) = source_rows.checked_add(resolver_marker_count) else {
            return false;
        };
        event_count == self.atom_count()
            && scope_marker_count == self.scope_marker_count()
            && resolver_marker_count <= self.route_resolver_count()
            && source_rows <= self.blob_len()
    }

    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        if self.has_ownership() {
            self.tail as usize
        } else {
            self.scope_markers()
                .end_offset(PROGRAM_IMAGE_SCOPE_MARKER_STRIDE)
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramImagePlan {
    columns: ProgramImageColumns,
    index_bytes: usize,
}

impl ProgramImagePlan {
    pub(crate) const fn from_source<const E: usize>(
        source: &crate::g::ProgramSourceData<E>,
    ) -> Self {
        let mut plan = Self::from_program(source.eff_list());
        if let Some(actors) = source.actors() {
            if actors.event_count() != plan.columns.atom_count() {
                crate::invariant();
            }
            let first_transfers = first_endpoint_transfers(source.eff_list());
            let mut initial_actors = 0;
            let mut role = 0u16;
            while role <= u8::MAX as u16 {
                if first_transfers[role as usize].is_some() {
                    initial_actors += 1;
                }
                role += 1;
            }
            plan.columns = match plan
                .columns
                .with_actors(actors.decision_count(), initial_actors)
            {
                Some(columns) => columns,
                None => panic!("owned program image exceeds compact offset domain"),
            };
        }
        plan
    }

    #[inline(always)]
    pub(crate) const fn from_program<const E: usize>(eff_list: &EffList<E>) -> Self {
        Self {
            columns: program_image_columns(eff_list),
            index_bytes: index_byte_len(eff_list),
        }
    }

    #[inline(always)]
    pub(crate) const fn columns(self) -> ProgramImageColumns {
        self.columns
    }

    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        self.columns.blob_len() + self.index_bytes
    }
}

pub(super) const fn index_byte_len<const E: usize>(eff_list: &EffList<E>) -> usize {
    let markers = eff_list.scope_markers();
    let mut extent = 0;
    let mut rolls = false;
    let mut resolver_extent = 0;
    let mut marker = 0;
    while marker < markers.len() {
        let scope = markers.at(marker).scope_id;
        if scope.kind().is_some() {
            if matches!(scope.kind(), Some(ScopeKind::Roll)) {
                rolls = true;
            }
            let end = scope.local_ordinal() as usize + 1;
            if matches!(scope.kind(), Some(ScopeKind::Route)) && end > resolver_extent {
                resolver_extent = end;
            }
            if end > extent {
                extent = end;
            }
        }
        marker += 1;
    }
    if extent == 0 {
        0
    } else {
        extent * 9 + eff_list.len() * if rolls { 4 } else { 2 } + resolver_extent * 2
    }
}

pub(super) const fn first_endpoint_transfers<const E: usize>(
    eff: &EffList<E>,
) -> [Option<usize>; 256] {
    let mut first = [None; 256];
    let mut event = 0;
    while event < eff.len() {
        if let crate::eff::EventPayload::Endpoint(role) = eff.atom_at(event).payload
            && first[role as usize].is_none()
        {
            first[role as usize] = Some(event);
        }
        event += 1;
    }
    first
}

#[cfg(all(test, hibana_repo_tests))]
mod source_summary_tests {
    use super::*;
    use crate::eff::{EffAtom, EventOrigin, EventPayload};

    #[test]
    fn transfer_and_participant_summaries_match_source_scans() {
        let mut events = EffList::<512>::new();
        for at in 0..512 {
            events.push_event_mut(EffAtom {
                from: at as u8,
                to: (at as u8).wrapping_mul(17),
                lane: 0,
                label: at as u8,
                origin: EventOrigin::User,
                payload: if at % 3 == 0 {
                    EventPayload::Wire(0)
                } else {
                    EventPayload::Endpoint((at as u8).wrapping_mul(31))
                },
            });
        }
        let first = first_endpoint_transfers(&events);
        for role in 0..=255 {
            assert_eq!(
                first[role as usize],
                (0..events.len())
                    .find(|&at| events.atom_at(at).payload == EventPayload::Endpoint(role))
            );
        }
        for start in [0, 1, 63, 127, 128, 255, 256, 511, 512] {
            for stop in start..=events.len() {
                let controller = 247;
                let actual = route_arm_participants(&events, (start, stop), controller);
                let mut expected = [false; 256];
                expected[controller as usize] = true;
                for at in start..stop {
                    let row = events.atom_at(at);
                    expected[row.from as usize] = true;
                    expected[row.to as usize] = true;
                }
                for role in 0..=255 {
                    assert_eq!(actual[role / 8] & (1 << (role % 8)) != 0, expected[role]);
                }
                if start != stop {
                    assert_eq!(
                        route_arm_participant_count(&events, (start, stop), controller),
                        expected.iter().filter(|&&present| present).count()
                    );
                }
            }
        }
    }
}

#[inline]
const fn program_image_columns<const E: usize>(eff_list: &EffList<E>) -> ProgramImageColumns {
    let atom_len = eff_list.len();

    let markers = eff_list.scope_markers();
    let mut route_resolver_len = 0usize;
    let mut route_participant_len = 0usize;
    let mut idx = 0;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            route_resolver_len += 1;
            let [(left_start, left_end), (right_start, right_end)] =
                route_arm_ranges_from_first_enter(markers, idx);
            let controller = eff_list.atom_at(left_start).from;
            route_participant_len +=
                route_arm_participant_count(eff_list, (left_start, left_end), controller);
            route_participant_len +=
                route_arm_participant_count(eff_list, (right_start, right_end), controller);
        }
        idx += 1;
    }

    ProgramImageColumns::new(
        atom_len,
        route_resolver_len,
        route_participant_len,
        markers.len(),
    )
}

pub(super) const fn route_arm_participants<const E: usize>(
    eff_list: &EffList<E>,
    (start, end): (usize, usize),
    controller: u8,
) -> [u8; 32] {
    let mut participants = [0; 32];
    // The canonical controlling view retains the choice even when another
    // co-resident view sends the selected arm's first message.
    participants[controller as usize / 8] |= 1 << (controller % 8);
    let mut event = start;
    while event < end && event < eff_list.len() {
        let atom = eff_list.atom_at(event);
        participants[atom.from as usize / 8] |= 1 << (atom.from % 8);
        participants[atom.to as usize / 8] |= 1 << (atom.to % 8);
        event += 1;
    }
    participants
}

const fn route_arm_participant_count<const E: usize>(
    eff_list: &EffList<E>,
    range: (usize, usize),
    controller: u8,
) -> usize {
    let participants = route_arm_participants(eff_list, range, controller);
    let mut count = 0;
    let mut byte = 0;
    while byte < participants.len() {
        count += participants[byte].count_ones() as usize;
        byte += 1;
    }
    if count == 0 {
        crate::invariant();
    }
    count
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramImageFacts {
    pub(crate) max_role: u8,
}

impl ProgramImageFacts {
    #[inline(always)]
    pub(crate) const fn from_image(image: &CompiledProgramImage) -> Self {
        Self {
            max_role: image.max_role(),
        }
    }
}
