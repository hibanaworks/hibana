mod common;
#[path = "support/frame_payload.rs"]
mod frame_payload;
#[path = "support/pending_cancel.rs"]
mod pending_cancel;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    future::Future,
    task::{Context, Poll},
};

use frame_payload::FramePayload;
use hibana::{
    g::{self, Msg},
    runtime::{
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use pending_cancel::{PENDING_CANCEL_SESSION_SLOT, PendingCancelTransport};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

#[path = "cursor_send_recv/session_fault_cancel.rs"]
mod session_fault_cancel;
