//! Const helpers for building segmented `EffStruct` images at compile time.
//!
//! These helpers lower global combinators (`send/seq/par/route`) into a
//! segment-addressed `EffList` read through crate-private accessors.
//!
//! # Unsafe Owner Contract
//!
//! `EffList` owns fixed arrays of compile-time metadata markers. The only raw
//! slice construction in this module exposes initialized prefixes whose lengths
//! are advanced by the same const builder methods that write the backing rows.
//! No returned slice outlives `self`, and no method exposes mutable aliases to
//! those rows while a shared prefix view exists.
mod eff_list;
mod route;

use crate::eff::{self, EffStruct};
use crate::global::Message;

const MAX_SEGMENT_EFFS: usize = eff::meta::MAX_SEGMENT_EFFS;
const MAX_SEGMENTS: usize = eff::meta::MAX_SEGMENTS;
const MAX_CAPACITY: usize = eff::meta::MAX_EFF_NODES;

mod scope;

pub(crate) use self::eff_list::const_send_typed;
pub(crate) use self::route::{
    ReentryMark, RouteFrontierSummary, RouteResolver, RouteResolverMarker,
};
pub(crate) use self::scope::{ScopeEvent, ScopeId, ScopeKind};

pub(crate) const INTRINSIC_ROUTE_RESOLVER_ID: u16 = u16::MAX;

#[derive(Clone, Copy)]
pub(crate) struct ScopeMarker {
    pub(crate) offset: usize,
    pub(crate) scope_id: ScopeId,
    pub(crate) event: ScopeEvent,
    pub(crate) reentry: ReentryMark,
}

impl ScopeMarker {
    pub(crate) const fn empty() -> Self {
        Self {
            offset: 0,
            scope_id: ScopeId::none(),
            event: ScopeEvent::Enter,
            reentry: ReentryMark::SinglePass,
        }
    }
}

/// Segment-local summary for effect rows and metadata markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SegmentSummary {
    eff_len: u16,
    scope_marker_len: u16,
    route_scope_enter_len: u16,
}

impl SegmentSummary {
    pub(crate) const EMPTY: Self = Self {
        eff_len: 0,
        scope_marker_len: 0,
        route_scope_enter_len: 0,
    };

    #[inline(always)]
    const fn bump(value: u16) -> u16 {
        if value == u16::MAX {
            panic!("segment summary overflow");
        }
        value + 1
    }

    #[inline(always)]
    const fn with_effect(mut self) -> Self {
        self.eff_len = Self::bump(self.eff_len);
        self
    }

    #[inline(always)]
    const fn with_scope_marker(mut self, scope_id: ScopeId, event: ScopeEvent) -> Self {
        self.scope_marker_len = Self::bump(self.scope_marker_len);
        if matches!(scope_id.kind(), Some(ScopeKind::Route)) && matches!(event, ScopeEvent::Enter) {
            self.route_scope_enter_len = Self::bump(self.route_scope_enter_len);
        }
        self
    }
}

/// Accumulator used to build `EffStruct` sequences in const contexts.
#[derive(Clone, Copy)]
pub(crate) struct EffList {
    segments: [[EffStruct; MAX_SEGMENT_EFFS]; MAX_SEGMENTS],
    segment_summaries: [SegmentSummary; MAX_SEGMENTS],
    len: usize,
    scope_budget: u16,
    scope_markers: [ScopeMarker; MAX_CAPACITY],
    scope_marker_len: usize,
    resolver_markers: [RouteResolverMarker; MAX_CAPACITY],
    resolver_marker_len: usize,
    route_frontiers: [RouteFrontierSummary; MAX_CAPACITY],
    route_frontier_len: usize,
}
