use super::{
    CompiledProgramImage, LANE_DOMAIN_BYTES, LaneSetView, LaneSteps, MAX_LOCAL_STEP_LANES,
    MAX_PHASE_BOUNDARY_ROWS, MAX_PHASE_LANE_ROWS, MAX_RESIDENT_LANE_BIT_BYTES,
    MAX_ROUTE_ARM_LANE_ROWS, MAX_ROUTE_SCOPE_LANE_ROWS, PackedLaneRange, RoleCompiledCounts,
    RoleFacts, RoleFootprint, RoleImage, RoleImageRef, RoleImageSource, RoleLaneImage, ScopeEvent,
    ScopeId, ScopeKind, ScopeMarker, lane_byte_count, lane_byte_index, lane_word_count,
};
impl RoleImage {
    #[inline(always)]
    pub(crate) const fn new(
        facts: RoleFacts,
        source: RoleImageSource,
        lanes: RoleLaneImage,
    ) -> Self {
        Self {
            facts,
            source,
            lanes,
        }
    }
}

impl RoleLaneImage {
    const NO_ACTIVE_LANE: u16 = u16::MAX;

    #[inline(always)]
    const fn same_scope(left: ScopeId, right: ScopeId) -> bool {
        !left.is_none() && left.canonical_raw() == right.canonical_raw()
    }

    #[inline(always)]
    const fn first_enter_for_scope(markers: &[ScopeMarker], marker_idx: usize) -> bool {
        let marker = markers[marker_idx];
        if !matches!(marker.event, ScopeEvent::Enter) {
            return false;
        }
        let mut idx = 0usize;
        while idx < marker_idx {
            let candidate = markers[idx];
            if matches!(candidate.event, ScopeEvent::Enter)
                && Self::same_scope(candidate.scope_id, marker.scope_id)
            {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    const fn route_arm_ranges(
        markers: &[ScopeMarker],
        route: ScopeId,
    ) -> Option<[(usize, usize); 2]> {
        if route.is_none() {
            return None;
        }
        let mut starts = [usize::MAX; 2];
        let mut ends = [usize::MAX; 2];
        let mut enter_len = 0usize;
        let mut exit_len = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if Self::same_scope(marker.scope_id, route)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                match marker.event {
                    ScopeEvent::Enter => {
                        if enter_len < 2 {
                            starts[enter_len] = marker.offset;
                        }
                        enter_len += 1;
                    }
                    ScopeEvent::Exit => {
                        if exit_len < 2 {
                            ends[exit_len] = marker.offset;
                        }
                        exit_len += 1;
                    }
                }
            }
            idx += 1;
        }
        if enter_len == 2 && exit_len == 2 {
            Some([(starts[0], ends[0]), (starts[1], ends[1])])
        } else {
            None
        }
    }

    #[inline(always)]
    const fn local_step_range_for_eff_range<const ROLE: u8>(
        program: &CompiledProgramImage,
        start_eff: usize,
        end_eff: usize,
    ) -> PackedLaneRange {
        if start_eff >= end_eff {
            return PackedLaneRange::new(0, 0);
        }
        let view = program.view();
        let mut local_step = 0usize;
        let mut local_start = usize::MAX;
        let mut local_len = 0usize;
        let mut eff_idx = 0usize;
        while eff_idx < view.len() {
            if let Some(atom) = view.atom_at(eff_idx) {
                if atom.from == ROLE || atom.to == ROLE {
                    if eff_idx >= start_eff && eff_idx < end_eff {
                        if local_start == usize::MAX {
                            local_start = local_step;
                        }
                        local_len += 1;
                    }
                    local_step += 1;
                }
            }
            eff_idx += 1;
        }
        if local_start == usize::MAX {
            PackedLaneRange::new(0, 0)
        } else {
            PackedLaneRange::new(local_start, local_len)
        }
    }

    #[inline(always)]
    const fn push_phase_row(&mut self, row: PackedLaneRange) {
        if row.len() == 0 {
            return;
        }
        let idx = self.phase_row_len as usize;
        if idx >= MAX_PHASE_LANE_ROWS {
            panic!("role phase lane row overflow");
        }
        if row.start() > u16::MAX as usize || row.end() > u16::MAX as usize {
            panic!("role phase lane row range overflow");
        }
        let start = row.start() as u16;
        let end = row.end() as u16;
        if idx == 0 {
            self.phase_boundaries[0] = start;
        } else if self.phase_boundaries[idx] != start {
            panic!("role phase lane rows must be contiguous");
        }
        self.phase_boundaries[idx + 1] = end;
        self.phase_row_len += 1;
    }

    #[inline(always)]
    const fn append_lane_bit_row_for_local_range(
        &mut self,
        row: PackedLaneRange,
    ) -> PackedLaneRange {
        if row.is_empty() || row.len() == 0 {
            return PackedLaneRange::new(0, 0);
        }
        if row.end() > MAX_LOCAL_STEP_LANES {
            panic!("resident lane bit row exceeds local lane table");
        }

        let mut bytes = [0u8; LANE_DOMAIN_BYTES];
        let mut max_lane_plus_one = 0usize;
        let mut pos = row.start();
        let end = row.end();
        while pos < end {
            let lane = self.local_step_lanes[pos] as usize;
            let (byte_idx, bit) = lane_byte_index(lane);
            bytes[byte_idx] |= bit;
            let lane_plus_one = lane.saturating_add(1);
            if lane_plus_one > max_lane_plus_one {
                max_lane_plus_one = lane_plus_one;
            }
            pos += 1;
        }

        let byte_len = lane_byte_count(max_lane_plus_one);
        if byte_len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let start = self.lane_bit_row_len as usize;
        let end = start.saturating_add(byte_len);
        if end > MAX_RESIDENT_LANE_BIT_BYTES || end > u16::MAX as usize {
            panic!("resident lane bit row overflow");
        }
        let mut idx = 0usize;
        while idx < byte_len {
            self.lane_bit_rows[start + idx] = bytes[idx];
            idx += 1;
        }
        self.lane_bit_row_len = end as u16;
        PackedLaneRange::new(start, byte_len)
    }

    #[inline(always)]
    const fn lane_bit_row_byte(&self, row: PackedLaneRange, idx: usize) -> u8 {
        if row.is_empty() || idx >= row.len() {
            0
        } else {
            let offset = row.start().saturating_add(idx);
            if offset >= MAX_RESIDENT_LANE_BIT_BYTES {
                0
            } else {
                self.lane_bit_rows[offset]
            }
        }
    }

    #[inline(always)]
    const fn append_lane_bit_union_row(
        &mut self,
        left: PackedLaneRange,
        right: PackedLaneRange,
    ) -> PackedLaneRange {
        let byte_len = if left.len() > right.len() {
            left.len()
        } else {
            right.len()
        };
        if byte_len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let start = self.lane_bit_row_len as usize;
        let end = start.saturating_add(byte_len);
        if end > MAX_RESIDENT_LANE_BIT_BYTES || end > u16::MAX as usize {
            panic!("resident lane bit union row overflow");
        }
        let mut idx = 0usize;
        while idx < byte_len {
            self.lane_bit_rows[start + idx] =
                self.lane_bit_row_byte(left, idx) | self.lane_bit_row_byte(right, idx);
            idx += 1;
        }
        self.lane_bit_row_len = end as u16;
        PackedLaneRange::new(start, byte_len)
    }

    #[inline(always)]
    const fn push_phase_lane_bit_rows(&mut self) {
        if self.phase_row_len == 0 {
            return;
        }
        let mut idx = 0usize;
        while idx < self.phase_row_len as usize {
            let bit_row = self.append_lane_bit_row_for_local_range(self.phase_range(idx));
            let start = bit_row.start();
            let end = bit_row.end();
            if start > u16::MAX as usize || end > u16::MAX as usize {
                panic!("resident phase lane bit row overflow");
            }
            if idx == 0 {
                self.phase_lane_bit_boundaries[0] = start as u16;
            } else if self.phase_lane_bit_boundaries[idx] != start as u16 {
                panic!("resident phase lane bit rows must be contiguous");
            }
            self.phase_lane_bit_boundaries[idx + 1] = end as u16;
            idx += 1;
        }
    }

    #[inline(always)]
    const fn push_phase_rows<const ROLE: u8>(&mut self, program: &CompiledProgramImage) {
        let view = program.view();
        let markers = view.scope_markers();
        let mut current_eff = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Parallel)
            {
                let mut exit_eff = usize::MAX;
                let mut scan = marker_idx + 1;
                while scan < markers.len() {
                    let candidate = markers[scan];
                    if Self::same_scope(candidate.scope_id, marker.scope_id)
                        && matches!(candidate.event, ScopeEvent::Exit)
                    {
                        exit_eff = candidate.offset;
                        break;
                    }
                    scan += 1;
                }
                if exit_eff == usize::MAX {
                    panic!("parallel scope exit missing");
                }
                self.push_phase_row(Self::local_step_range_for_eff_range::<ROLE>(
                    program,
                    current_eff,
                    marker.offset,
                ));
                let parallel_start = if marker.offset > current_eff {
                    marker.offset
                } else {
                    current_eff
                };
                self.push_phase_row(Self::local_step_range_for_eff_range::<ROLE>(
                    program,
                    parallel_start,
                    exit_eff,
                ));
                current_eff = if exit_eff > current_eff {
                    exit_eff
                } else {
                    current_eff
                };
            }
            marker_idx += 1;
        }
        self.push_phase_row(Self::local_step_range_for_eff_range::<ROLE>(
            program,
            current_eff,
            view.len(),
        ));
        if self.phase_row_len == 0 {
            self.push_phase_row(Self::local_step_range_for_eff_range::<ROLE>(
                program,
                0,
                view.len(),
            ));
        }
    }

    #[inline(always)]
    const fn append_route_arm_lane_row<const ROLE: u8>(
        &mut self,
        program: &CompiledProgramImage,
        slot: usize,
        arm: usize,
        start_eff: usize,
        end_eff: usize,
    ) {
        let row_idx = slot.saturating_mul(2).saturating_add(arm);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            panic!("route arm lane row overflow");
        }
        let local_row = Self::local_step_range_for_eff_range::<ROLE>(program, start_eff, end_eff);
        self.route_arm_lane_rows[row_idx] = self.append_lane_bit_row_for_local_range(local_row);
    }

    #[inline(always)]
    const fn push_route_arm_lane_rows<const ROLE: u8>(&mut self, program: &CompiledProgramImage) {
        let view = program.view();
        let markers = view.scope_markers();
        let mut route_slot = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if Self::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                let Some(ranges) = Self::route_arm_ranges(markers, marker.scope_id) else {
                    panic!("route scope missing binary arm ranges");
                };
                let mut arm = 0usize;
                while arm < 2 {
                    let (start, end) = ranges[arm];
                    self.append_route_arm_lane_row::<ROLE>(program, route_slot, arm, start, end);
                    arm += 1;
                }
                if route_slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
                    panic!("route offer lane row overflow");
                }
                let left = self.route_arm_lane_rows[route_slot.saturating_mul(2)];
                let right =
                    self.route_arm_lane_rows[route_slot.saturating_mul(2).saturating_add(1)];
                self.route_offer_lane_rows[route_slot] =
                    self.append_lane_bit_union_row(left, right);
                route_slot += 1;
            }
            marker_idx += 1;
        }
    }

    #[inline(always)]
    pub(crate) const fn from_program<const ROLE: u8>(
        program: &CompiledProgramImage,
        logical_lane_count: usize,
    ) -> Self {
        let mut lanes = Self {
            local_step_lanes: [0; MAX_LOCAL_STEP_LANES],
            phase_boundaries: [0; MAX_PHASE_BOUNDARY_ROWS],
            phase_lane_bit_boundaries: [0; MAX_PHASE_BOUNDARY_ROWS],
            lane_bit_rows: [0; MAX_RESIDENT_LANE_BIT_BYTES],
            route_arm_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            route_offer_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_SCOPE_LANE_ROWS],
            active_lane_row: PackedLaneRange::EMPTY,
            phase_row_len: 0,
            lane_bit_row_len: 0,
            first_active_lane: Self::NO_ACTIVE_LANE,
        };
        let view = program.view();
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                if atom.from == ROLE || atom.to == ROLE {
                    let lane = atom.lane as usize;
                    if lane < logical_lane_count {
                        if lane < lanes.first_active_lane as usize {
                            lanes.first_active_lane = lane as u16;
                        }
                        if step >= MAX_LOCAL_STEP_LANES {
                            panic!("role local lane table overflow");
                        }
                        lanes.local_step_lanes[step] = atom.lane;
                    }
                    step += 1;
                }
            }
            idx += 1;
        }
        lanes.active_lane_row =
            lanes.append_lane_bit_row_for_local_range(PackedLaneRange::new(0, step));
        lanes.push_phase_rows::<ROLE>(program);
        lanes.push_phase_lane_bit_rows();
        lanes.push_route_arm_lane_rows::<ROLE>(program);
        lanes
    }

    #[inline(always)]
    const fn lane_bit_view(&self, range: PackedLaneRange, word_len: usize) -> LaneSetView<'_> {
        if range.is_empty() || range.len() == 0 {
            LaneSetView::from_bytes(core::ptr::null(), 0, word_len)
        } else {
            if range.end() > MAX_RESIDENT_LANE_BIT_BYTES {
                panic!("resident lane bit range exceeds lane bit table");
            }
            LaneSetView::from_bytes(
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe { self.lane_bit_rows.as_ptr().add(range.start()) },
                range.len(),
                word_len,
            )
        }
    }

    #[inline(always)]
    const fn active_lane_set(&self, word_len: usize) -> LaneSetView<'_> {
        self.lane_bit_view(self.active_lane_row, word_len)
    }

    #[inline(always)]
    const fn phase_lane_set(&self, idx: usize, word_len: usize) -> Option<LaneSetView<'_>> {
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let start = self.phase_lane_bit_boundaries[idx] as usize;
        let end = self.phase_lane_bit_boundaries[idx + 1] as usize;
        Some(self.lane_bit_view(
            PackedLaneRange::new(start, end.saturating_sub(start)),
            word_len,
        ))
    }

    #[inline(always)]
    const fn phase_min_start(&self, idx: usize) -> Option<u16> {
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let row = self.phase_range(idx);
        if row.is_empty() || row.len() == 0 {
            None
        } else if row.start() > u16::MAX as usize {
            panic!("phase start exceeds descriptor capacity");
        } else {
            Some(row.start() as u16)
        }
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_steps(&self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let row = self.phase_range(idx);
        let mut pos = row.start();
        let end = row.end();
        let mut first = usize::MAX;
        let mut len = 0usize;
        let mut sparse = false;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if first == usize::MAX {
                    first = pos;
                } else if pos != first.saturating_add(len) {
                    sparse = true;
                }
                len += 1;
            }
            pos += 1;
        }
        if len == 0 {
            None
        } else if first > u16::MAX as usize || len > u16::MAX as usize {
            panic!("phase lane steps exceed descriptor capacity");
        } else {
            Some(LaneSteps {
                start: first as u16,
                len: len as u16,
                sparse,
            })
        }
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_step_at(
        &self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let row = self.phase_range(idx);
        let mut pos = row.start();
        let end = row.end();
        let mut seen = 0usize;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if seen == ordinal {
                    if pos > u16::MAX as usize {
                        panic!("phase lane step index exceeds descriptor capacity");
                    }
                    return Some(pos as u16);
                }
                seen += 1;
            }
            pos += 1;
        }
        None
    }

    #[inline(always)]
    const fn phase_lane_step_ordinal(
        &self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let row = self.phase_range(idx);
        if step_idx < row.start() || step_idx >= row.end() || step_idx >= MAX_LOCAL_STEP_LANES {
            return None;
        }
        let mut pos = row.start();
        let end = row.end();
        let mut ordinal = 0usize;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if pos == step_idx {
                    if ordinal > u16::MAX as usize {
                        panic!("phase lane step ordinal exceeds descriptor capacity");
                    }
                    return Some(ordinal as u16);
                }
                ordinal += 1;
            }
            pos += 1;
        }
        None
    }

    #[inline(always)]
    const fn first_active_lane(&self) -> Option<usize> {
        if self.first_active_lane == Self::NO_ACTIVE_LANE {
            None
        } else {
            Some(self.first_active_lane as usize)
        }
    }

    #[inline(always)]
    const fn phase_range(&self, idx: usize) -> PackedLaneRange {
        if idx >= self.phase_row_len as usize {
            return PackedLaneRange::EMPTY;
        }
        let start = self.phase_boundaries[idx] as usize;
        let end = self.phase_boundaries[idx + 1] as usize;
        PackedLaneRange::new(start, end.saturating_sub(start))
    }

    #[inline(always)]
    const fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
        logical_lane_word_count: usize,
    ) -> Option<LaneSetView<'_>> {
        if arm >= 2 {
            return None;
        }
        let row_idx = slot.saturating_mul(2).saturating_add(arm as usize);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            return None;
        }
        let row = self.route_arm_lane_rows[row_idx];
        if row.is_empty() {
            return None;
        }
        Some(self.lane_bit_view(row, logical_lane_word_count))
    }

    #[inline(always)]
    const fn route_scope_offer_lane_set_by_slot(
        &self,
        slot: usize,
        logical_lane_word_count: usize,
    ) -> Option<LaneSetView<'_>> {
        if slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
            return None;
        }
        let row = self.route_offer_lane_rows[slot];
        if row.is_empty() {
            return None;
        }
        Some(self.lane_bit_view(row, logical_lane_word_count))
    }
}

impl RoleFacts {
    #[cfg(test)]
    const SCOPE_COUNT: usize = 0;
    #[cfg(test)]
    const MAX_ACTIVE_SCOPE_DEPTH: usize = 1;
    const MAX_ROUTE_STACK_DEPTH: usize = 2;
    #[cfg(test)]
    const EFF_COUNT: usize = 3;
    const LOCAL_STEP_COUNT: usize = 4;
    #[cfg(test)]
    const PHASE_COUNT: usize = 5;
    #[cfg(test)]
    const PHASE_LANE_ENTRY_COUNT: usize = 6;
    #[cfg(test)]
    const PHASE_LANE_WORD_COUNT: usize = 7;
    #[cfg(test)]
    const PARALLEL_ENTER_COUNT: usize = 8;
    const ROUTE_SCOPE_COUNT: usize = 9;
    const PASSIVE_LINGER_ROUTE_SCOPE_COUNT: usize = 10;
    const ACTIVE_LANE_COUNT: usize = 11;
    const ENDPOINT_LANE_SLOT_COUNT: usize = 12;
    const LOGICAL_LANE_COUNT: usize = 13;

    #[inline(always)]
    const fn compact_count(value: usize) -> u16 {
        if value > u16::MAX as usize {
            panic!("role descriptor fact overflow");
        }
        value as u16
    }

    #[inline(always)]
    pub(crate) const fn from_counts(counts: RoleCompiledCounts) -> Self {
        Self {
            words: [
                Self::compact_count(counts.scope_count),
                Self::compact_count(counts.max_active_scope_depth),
                Self::compact_count(counts.max_route_stack_depth),
                Self::compact_count(counts.eff_count),
                Self::compact_count(counts.local_step_count),
                Self::compact_count(counts.phase_count),
                Self::compact_count(counts.phase_lane_entry_count),
                Self::compact_count(counts.phase_lane_word_count),
                Self::compact_count(counts.parallel_enter_count),
                Self::compact_count(counts.route_scope_count),
                Self::compact_count(counts.passive_linger_route_scope_count),
                Self::compact_count(counts.active_lane_count),
                Self::compact_count(counts.endpoint_lane_slot_count),
                Self::compact_count(counts.logical_lane_count),
            ],
        }
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RoleFootprint {
        RoleFootprint {
            #[cfg(test)]
            scope_count: self.words[Self::SCOPE_COUNT] as usize,
            #[cfg(test)]
            max_active_scope_depth: self.words[Self::MAX_ACTIVE_SCOPE_DEPTH] as usize,
            max_route_stack_depth: self.words[Self::MAX_ROUTE_STACK_DEPTH] as usize,
            #[cfg(test)]
            eff_count: self.words[Self::EFF_COUNT] as usize,
            #[cfg(test)]
            phase_count: self.words[Self::PHASE_COUNT] as usize,
            #[cfg(test)]
            phase_lane_entry_count: self.words[Self::PHASE_LANE_ENTRY_COUNT] as usize,
            #[cfg(test)]
            phase_lane_word_count: self.words[Self::PHASE_LANE_WORD_COUNT] as usize,
            #[cfg(test)]
            parallel_enter_count: self.words[Self::PARALLEL_ENTER_COUNT] as usize,
            route_scope_count: self.words[Self::ROUTE_SCOPE_COUNT] as usize,
            local_step_count: self.words[Self::LOCAL_STEP_COUNT] as usize,
            passive_linger_route_scope_count: self.words[Self::PASSIVE_LINGER_ROUTE_SCOPE_COUNT]
                as usize,
            active_lane_count: self.words[Self::ACTIVE_LANE_COUNT] as usize,
            endpoint_lane_slot_count: self.words[Self::ENDPOINT_LANE_SLOT_COUNT] as usize,
            logical_lane_count: self.words[Self::LOGICAL_LANE_COUNT] as usize,
            logical_lane_word_count: lane_word_count(self.words[Self::LOGICAL_LANE_COUNT] as usize),
            scope_evidence_count: self.words[Self::ROUTE_SCOPE_COUNT] as usize,
            frontier_entry_count: RoleFootprint::frontier_entry_count_for_route_depth(
                self.words[Self::MAX_ROUTE_STACK_DEPTH] as usize,
            ),
        }
    }
}

impl RoleImageRef {
    #[inline(always)]
    pub(crate) const fn new(image: &'static RoleImage) -> Self {
        Self { image }
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RoleFootprint {
        self.image.facts.footprint()
    }

    #[inline(always)]
    pub(crate) fn program_image(self) -> &'static CompiledProgramImage {
        self.image.source.program_image()
    }

    #[inline(always)]
    pub(crate) const fn active_lane_set(self) -> LaneSetView<'static> {
        let footprint = self.footprint();
        self.image
            .lanes
            .active_lane_set(footprint.logical_lane_word_count)
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_set(self, idx: usize) -> Option<LaneSetView<'static>> {
        self.image
            .lanes
            .phase_lane_set(idx, self.footprint().logical_lane_word_count)
    }

    #[inline(always)]
    pub(crate) const fn phase_min_start(self, idx: usize) -> Option<u16> {
        self.image.lanes.phase_min_start(idx)
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_steps(self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        self.image.lanes.phase_lane_steps(idx, lane_idx)
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_step_at(
        self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        self.image.lanes.phase_lane_step_at(idx, lane_idx, ordinal)
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_step_ordinal(
        self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        self.image
            .lanes
            .phase_lane_step_ordinal(idx, lane_idx, step_idx)
    }

    #[inline(always)]
    pub(crate) const fn first_active_lane(self) -> Option<usize> {
        self.image.lanes.first_active_lane()
    }

    #[inline(always)]
    pub(crate) const fn route_scope_arm_lane_set_by_slot(
        self,
        slot: usize,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.image.lanes.route_scope_arm_lane_set_by_slot(
            slot,
            arm,
            self.footprint().logical_lane_word_count,
        )
    }

    #[inline(always)]
    pub(crate) const fn route_scope_offer_lane_set_by_slot(
        self,
        slot: usize,
    ) -> Option<LaneSetView<'static>> {
        self.image
            .lanes
            .route_scope_offer_lane_set_by_slot(slot, self.footprint().logical_lane_word_count)
    }
}
