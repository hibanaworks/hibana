//! Role-local typestate synthesis derived from `EffList`.
//!
//! This module materialises compact role-local transition facts from an
//! `EffList`. Each node captures the local action (send/recv/control) together
//! with the successor index, allowing higher layers to drive endpoint
//! transitions from compiled facts.

mod cursor;
mod facts;

pub(crate) use self::facts::StateIndex;
pub(crate) use self::{
    cursor::{
        CursorRefresh, LoopMetadata, LoopRole, PhaseCursor, PhaseCursorState, ResidentLaneStep,
        ResidentLaneStepError,
    },
    facts::{
        ARM_SHARED, FirstRecvDispatchSpec, JumpReason, LocalAction, LocalAtomFacts, LocalMeta,
        LocalNode, LocalNodeMeta, MAX_FIRST_RECV_DISPATCH, MAX_STATES, PassiveArmNavigation,
        RecvMeta, ScopeRegion, SendMeta, state_index_to_usize,
    },
};
