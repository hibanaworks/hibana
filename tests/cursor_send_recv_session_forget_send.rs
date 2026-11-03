mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;
use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
#[path = "cursor_send_recv/session_forget_send.rs"]
mod session_forget_send;
