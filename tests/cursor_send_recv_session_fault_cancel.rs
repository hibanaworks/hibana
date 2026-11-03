mod common;
#[path = "support/frame_payload.rs"]
mod frame_payload;
#[path = "support/pending_cancel.rs"]
mod pending_cancel;

use core::{
    future::Future,
    task::{Context, Poll},
};

use frame_payload::FramePayload;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use pending_cancel::PendingCancelTransport;

#[path = "cursor_send_recv/session_fault_cancel.rs"]
mod session_fault_cancel;
