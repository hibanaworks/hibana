pub(crate) use core::cell::UnsafeCell;

pub(crate) use crate::common::TestTransport;
pub(crate) use crate::runtime_support::with_runtime_workspace;
pub(crate) use crate::tls_ref_support::with_resident_tls_ref;
pub(crate) use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
    runtime::{SessionKitStorage, ids::SessionId},
};

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

std::thread_local! {
    pub(crate) static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}
