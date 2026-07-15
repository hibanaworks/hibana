//! Const helpers for building exact-capacity source images at compile time.
//!
//! These helpers lower global combinators (`send/seq/par/route`) into a
//! `EffList` read through crate-private accessors.
//!
//! # Unsafe Owner Contract
//!
//! `EffList` owns one tagged arena partitioned by the exact event, scope-marker,
//! and resolver counts derived from the choreography type tree. Views decode
//! initialized rows by value, so source metadata needs no raw slices or aliases.
mod allocation;
mod eff_list;
mod endpoint_controller;
mod endpoint_selectors;
mod event_relations;
mod receive_lane_causality;
mod route;
mod scope_ranges;
mod source_arena;

use crate::eff;

mod scope;

pub(crate) use self::allocation::{
    color_roll_frame_labels, merge_parallel_lanes, merge_route_frame_labels,
};
#[cfg(all(test, hibana_repo_tests))]
pub(crate) use self::eff_list::const_send_typed;
pub(crate) use self::endpoint_controller::first_visible_controller;
pub(crate) use self::endpoint_selectors::{
    first_visible_endpoint_selector_conflicts_from_markers, local_route_observer_paths_mergeable,
    validate_parallel_endpoint_selectors, validate_roll_reentry_endpoint_selectors,
};
pub(crate) use self::receive_lane_causality::validate_receive_lane_causality;
pub(crate) use self::route::{DynamicRouteResolver, ReentryMark, RouteResolverMarker};
pub(crate) use self::scope::{ScopeEvent, ScopeId, ScopeKind};
pub(crate) use self::scope_ranges::{
    parallel_arm_ranges_from_enter, route_arm_ranges_from_first_enter,
};
use self::source_arena::SourceRow;
pub(crate) use self::source_arena::{EffList, ScopeMarker, ScopeMarkerView};

pub(crate) const INTRINSIC_ROUTE_RESOLVER_ID: u16 = u16::MAX;
