mod common;
#[path = "support/frame_payload.rs"]
mod frame_payload;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use core::{
    future::Future,
    task::{Context, Poll},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

use common::TestTransport;
use frame_payload::FramePayload;
use hibana::runtime::transport::{ReceivedFrame, Transport, TransportError};
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};

#[path = "cursor_send_recv/send_recv.rs"]
mod send_recv;
