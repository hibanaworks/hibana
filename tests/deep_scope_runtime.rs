#![recursion_limit = "512"]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    g,
    g::Msg,
    runtime::program::{RoleProgram, project},
    runtime::{SessionKitStorage, ids::SessionId},
};

type DeepScopeKitStorage<'a> = SessionKitStorage<'a, TestTransport>;

macro_rules! deep_nested_par_scope_program {
    ($($lane:literal)*) => {{
        let program = g::send::<0, 1, Msg<250, ()>>();
        $(
            let program = g::par(g::send::<2, 3, Msg<$lane, ()>>(), program);
        )*
        program
    }};
}

fn deep_active_scope_controller_program() -> RoleProgram<0> {
    let program = deep_nested_par_scope_program!(
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15
        16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
        32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47
        48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63
        64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79
        80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95
        96 97 98 99 100 101 102 103 104 105 106 107 108 109 110 111
        112 113 114 115 116 117 118 119 120 121 122 123 124 125 126 127
        128
    );
    project(&program)
}

#[test]
fn active_scope_depth_above_128_enters_public_sessionkit_path() {
    runtime_support::with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let mut kit_storage = DeepScopeKitStorage::uninit();
        let kit = kit_storage.init();
        let rv = kit
            .rendezvous(slab, transport)
            .expect("register deep-scope rendezvous");

        let controller = rv
            .enter(
                SessionId::new(0x6210),
                &deep_active_scope_controller_program(),
            )
            .expect("enter role with >128 active nested scopes");
        core::hint::black_box(&controller);
    });
}
