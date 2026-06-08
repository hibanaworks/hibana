//! Role-local typestate synthesis derived from `EffList`.
//!
//! This module materialises compact role-local transition facts from an
//! `EffList`. Each node captures the local action (send/recv/control) together
//! with the successor index, allowing higher layers to drive endpoint
//! transitions from compiled facts.

mod cursor;
mod facts;

#[cfg(all(test, hibana_repo_tests))]
pub(crate) use self::cursor::EnabledEventCommit;
pub(crate) use self::facts::LocalAction;
pub(crate) use self::facts::StateIndex;
pub(crate) use self::{
    cursor::{
        CursorRefresh, EventCursor, EventCursorState, FlowPreviewError, LoopMetadata, LoopRole,
        RelocatableResidentLaneStep, ResidentLaneStepError,
    },
    facts::{
        ARM_SHARED, JumpReason, LocalAtomFacts, LocalConflict, LocalDependency, LocalMeta,
        LocalNode, LocalNodeMeta, MAX_FIRST_RECV_DISPATCH, MAX_STATES, PackedEventConflict,
        PackedLocalDependency, PassiveArmChildRow, RecvMeta, RouteScopeRows, SendMeta,
        state_index_to_usize,
    },
};
