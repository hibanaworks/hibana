mod common;
#[path = "support/frame_payload.rs"]
mod frame_payload;

use core::{
    future::Future,
    task::{Context, Poll, Waker},
};

use frame_payload::FramePayload;

#[path = "cursor_send_recv/session_drop_wake.rs"]
mod session_drop_wake;

use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
