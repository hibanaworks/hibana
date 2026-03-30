//! Runtime management prefix and payload owners.
//!
//! The public runtime-facing surface here is limited to the management prefix
//! choreography owners and the typed payload/reply owners they carry. The
//! staging manager, promotion gate, and compiled-program test helpers live in
//! test-only support.

mod observe_stream;
mod payload;
mod request_reply;

#[cfg(test)]
mod test_support;

pub use observe_stream::{
    PROGRAM as OBSERVE_STREAM_PREFIX, ProgramSteps as ObserveStreamPrefixSteps,
};
pub use payload::{
    LoadBegin, LoadChunk, LoadReport, LoadRequest, MgmtError, Reply, Request, SlotRequest,
    StatsResp, SubscribeReq, TransitionReport,
};
pub use request_reply::{PROGRAM as REQUEST_REPLY_PREFIX, ProgramSteps as RequestReplyPrefixSteps};
pub use request_reply::{ROLE_CLUSTER, ROLE_CONTROLLER};

#[cfg(test)]
pub(crate) use test_support::with_management_compiled_programs_for_test;
