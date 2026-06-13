//! Role-local typestate synthesis derived from `EffList`.
//!
//! This module materialises compact role-local transition facts from an
//! `EffList`. Each node captures the local action (send/recv) together
//! with the successor index, allowing higher layers to drive endpoint
//! transitions from compiled facts.

mod cursor;
mod facts;

pub(crate) use self::facts::LocalAction;
pub(crate) use self::facts::StateIndex;
pub(crate) use self::{
    cursor::{
        CursorInvariantError, CursorRefresh, EventCursor, EventCursorState, FlowPreviewError,
        RelocatableResidentLaneStep,
    },
    facts::{
        ARM_SHARED, EventCommitMeta, LocalAtomFacts, LocalConflict, LocalDependency, LocalMeta,
        LocalNode, LocalNodeMeta, MAX_FIRST_RECV_DISPATCH, MAX_STATES, PackedEventConflict,
        PackedLocalDependency, PassiveArmChildFact, RecvMeta, RouteScopeRows, SendMeta,
        state_index_to_usize,
    },
};
