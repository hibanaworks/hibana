//! Internal endpoint kernel split by responsibility.

mod authority;
mod control;
mod core;
mod decode;
mod evidence;
mod evidence_store;
mod frontier;
mod frontier_state;
mod inbox;
mod lane_port;
mod observe;
mod offer;
mod recv;
mod route_state;
mod send;

#[allow(unused_imports)]
pub(super) use self::core::*;
pub(crate) use self::core::{CanonicalTokenProvider, CursorEndpoint, RouteBranch};
