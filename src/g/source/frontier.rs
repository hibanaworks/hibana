use crate::eff;
use crate::global::const_dsl::{EffList, RouteFrontierSummary, ScopeId};

const ENDPOINT_OP_FRONTIER_CAPACITY: usize = crate::global::role_program::LANE_DOMAIN_SIZE * 2;

#[derive(Clone, Copy)]
pub(super) struct EndpointOpFrontier {
    ops: [EndpointOpKey; ENDPOINT_OP_FRONTIER_CAPACITY],
    len: u16,
    controller_mask: u16,
    empty: bool,
    invalid: bool,
    pub(super) ambiguous_endpoint_op: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct EndpointOpKey(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
struct OutboundOpKey(u16);

#[derive(Clone, Copy, PartialEq, Eq)]
struct ProjectedInboundKey(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
enum EndpointOpKind {
    Outbound,
    Inbound,
}

impl EndpointOpKey {
    const EMPTY: Self = Self(0);
    const KIND_OUTBOUND: u32 = 1 << 31;
    const KIND_INBOUND: u32 = 1 << 30;
    const PAYLOAD_MASK: u32 = 0x00ff_ffff;

    const fn outbound(atom: eff::EffAtom) -> Self {
        Self(Self::KIND_OUTBOUND | OutboundOpKey::new(atom.from, atom.label).payload())
    }

    const fn inbound(atom: eff::EffAtom, frame_label: u8) -> Self {
        Self(
            Self::KIND_INBOUND
                | ProjectedInboundKey::new(atom.to, atom.lane, atom.from, frame_label).payload(),
        )
    }

    const fn kind(self) -> EndpointOpKind {
        if self.is_inbound() {
            EndpointOpKind::Inbound
        } else {
            EndpointOpKind::Outbound
        }
    }

    const fn is_inbound(self) -> bool {
        self.0 & Self::KIND_INBOUND != 0
    }

    const fn inbound_key(self) -> ProjectedInboundKey {
        ProjectedInboundKey(self.0 & Self::PAYLOAD_MASK)
    }

    const fn rebase_inbound_frame_label(self, delta: u8) -> Self {
        if !self.is_inbound() || delta == 0 {
            return self;
        }
        Self(Self::KIND_INBOUND | self.inbound_key().rebase_frame_label(delta).payload())
    }

    const fn rebase_inbound_lane(self, delta: u16) -> Self {
        if !self.is_inbound() || delta == 0 {
            return self;
        }
        Self(Self::KIND_INBOUND | self.inbound_key().rebase_lane(delta).payload())
    }

    const fn conflicts(self, other: Self) -> bool {
        self.0 == other.0
    }

    const fn inbound_local_role(self) -> u8 {
        self.inbound_key().local_role()
    }

    const fn inbound_lane(self) -> u8 {
        self.inbound_key().lane()
    }
}

impl OutboundOpKey {
    const fn new(local_role: u8, logical_label: u8) -> Self {
        if local_role >= crate::g::ROLE_DOMAIN_SIZE {
            panic!("role domain overflow");
        }
        Self(((local_role as u16) << 8) | logical_label as u16)
    }

    const fn payload(self) -> u32 {
        self.0 as u32
    }
}

impl ProjectedInboundKey {
    const fn new(local_role: u8, lane: u8, source_role: u8, frame_label: u8) -> Self {
        if local_role >= crate::g::ROLE_DOMAIN_SIZE || source_role >= crate::g::ROLE_DOMAIN_SIZE {
            panic!("role domain overflow");
        }
        Self(
            ((local_role as u32) << 20)
                | ((source_role as u32) << 16)
                | ((lane as u32) << 8)
                | frame_label as u32,
        )
    }

    const fn payload(self) -> u32 {
        self.0
    }

    const fn local_role(self) -> u8 {
        ((self.0 >> 20) & 0x0f) as u8
    }

    const fn lane(self) -> u8 {
        ((self.0 >> 8) & 0xff) as u8
    }

    const fn source_role(self) -> u8 {
        ((self.0 >> 16) & 0x0f) as u8
    }

    const fn frame_label(self) -> u8 {
        (self.0 & 0xff) as u8
    }

    const fn rebase_frame_label(self, delta: u8) -> Self {
        let frame_label = self.frame_label() as u16 + delta as u16;
        if frame_label > u8::MAX as u16 {
            panic!("frame label domain overflow");
        }
        Self::new(
            self.local_role(),
            self.lane(),
            self.source_role(),
            frame_label as u8,
        )
    }

    const fn rebase_lane(self, delta: u16) -> Self {
        let lane = self.lane() as u16 + delta;
        if lane > u8::MAX as u16 {
            panic!("projection internal lane overflow");
        }
        Self::new(
            self.local_role(),
            lane as u8,
            self.source_role(),
            self.frame_label(),
        )
    }
}

impl EndpointOpFrontier {
    pub(super) const EMPTY: Self = Self {
        ops: [EndpointOpKey::EMPTY; ENDPOINT_OP_FRONTIER_CAPACITY],
        len: 0,
        controller_mask: 0,
        empty: true,
        invalid: false,
        ambiguous_endpoint_op: false,
    };

    pub(super) const fn from_eff(eff: &EffList) -> Self {
        if eff.is_empty() {
            return Self::EMPTY;
        }
        let node = eff.node_at(0);
        if !matches!(node.kind, crate::eff::EffKind::Atom) {
            return Self::EMPTY;
        }
        Self::atom(node.atom_data())
    }

    const fn atom(atom: eff::EffAtom) -> Self {
        let invalid =
            atom.from >= crate::g::ROLE_DOMAIN_SIZE || atom.to >= crate::g::ROLE_DOMAIN_SIZE;
        let mut out = Self {
            ops: [EndpointOpKey::EMPTY; ENDPOINT_OP_FRONTIER_CAPACITY],
            len: 0,
            controller_mask: if invalid { 0 } else { 1u16 << atom.from },
            empty: false,
            invalid,
            ambiguous_endpoint_op: false,
        };
        if invalid {
            return out;
        }
        out = out.push_key(EndpointOpKey::outbound(atom));
        out.push_key(EndpointOpKey::inbound(atom, 0))
    }

    pub(super) const fn seq(self, next: Self) -> Self {
        if self.empty { next } else { self }
    }

    const fn push_key(mut self, key: EndpointOpKey) -> Self {
        if self.len as usize >= self.ops.len() {
            self.invalid = true;
            return self;
        }
        self.ops[self.len as usize] = key;
        self.len += 1;
        self
    }

    const fn has_conflict_with(self, key: EndpointOpKey) -> bool {
        let mut idx = 0usize;
        while idx < self.len as usize {
            if self.ops[idx].conflicts(key) {
                return true;
            }
            idx += 1;
        }
        false
    }

    const fn intersects(self, other: Self) -> bool {
        let mut idx = 0usize;
        while idx < other.len as usize {
            if self.has_conflict_with(other.ops[idx]) {
                return true;
            }
            idx += 1;
        }
        false
    }

    pub(super) const fn concurrent_union(self, other: Self) -> Self {
        let mut out = Self {
            ops: self.ops,
            len: self.len,
            controller_mask: self.controller_mask | other.controller_mask,
            empty: self.empty && other.empty,
            invalid: self.invalid || other.invalid,
            ambiguous_endpoint_op: self.ambiguous_endpoint_op
                || other.ambiguous_endpoint_op
                || self.intersects(other),
        };
        let mut idx = 0usize;
        while idx < other.len as usize {
            out = out.push_key(other.ops[idx]);
            idx += 1;
        }
        out
    }

    pub(super) const fn route_choice(self, other: Self, left_arm: &EffList) -> Self {
        let other = other.rebase_right_route_arm_inbound_frame_labels(left_arm);
        let mut out = Self {
            ops: self.ops,
            len: self.len,
            controller_mask: self.controller_mask | other.controller_mask,
            empty: self.empty && other.empty,
            invalid: self.invalid || other.invalid,
            ambiguous_endpoint_op: self.ambiguous_endpoint_op || other.ambiguous_endpoint_op,
        };
        let mut idx = 0usize;
        while idx < other.len as usize {
            out = out.push_key(other.ops[idx]);
            idx += 1;
        }
        out
    }

    pub(super) const fn rebase_parallel_inbound_lanes(mut self, delta: u16) -> Self {
        if delta == 0 {
            return self;
        }
        let mut idx = 0usize;
        while idx < self.len as usize {
            self.ops[idx] = self.ops[idx].rebase_inbound_lane(delta);
            idx += 1;
        }
        self
    }

    const fn rebase_right_route_arm_inbound_frame_labels(mut self, left_arm: &EffList) -> Self {
        let mut idx = 0usize;
        while idx < self.len as usize {
            let key = self.ops[idx];
            if matches!(key.kind(), EndpointOpKind::Inbound) {
                let delta = count_frame_labels_before_right_arm(
                    left_arm,
                    key.inbound_local_role(),
                    key.inbound_lane(),
                );
                self.ops[idx] = key.rebase_inbound_frame_label(delta);
            }
            idx += 1;
        }
        self
    }

    pub(super) const fn route_summary(
        scope: ScopeId,
        left_arm: &EffList,
        left: Self,
        right: Self,
    ) -> RouteFrontierSummary {
        let right = right.rebase_right_route_arm_inbound_frame_labels(left_arm);
        RouteFrontierSummary::new(
            scope,
            left.controller_mask | right.controller_mask,
            left.empty || right.empty || left.invalid || right.invalid,
            left.ambiguous_endpoint_op || right.ambiguous_endpoint_op,
            left.intersects(right),
        )
    }
}

const fn count_frame_labels_before_right_arm(left_arm: &EffList, target: u8, lane: u8) -> u8 {
    let mut count = 0usize;
    let mut idx = 0usize;
    while idx < left_arm.len() {
        let node = left_arm.node_at(idx);
        if matches!(node.kind, crate::eff::EffKind::Atom) {
            let atom = node.atom_data();
            if atom.to == target && atom.lane == lane {
                count += 1;
                if count > u8::MAX as usize {
                    panic!("frame label domain overflow");
                }
            }
        }
        idx += 1;
    }
    count as u8
}
