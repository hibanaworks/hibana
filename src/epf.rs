//! EPF — Effect Policy Filter (no_std / no_alloc).
//!
//! This layer brings together the EPF bytecode loader, verifier, dispatcher,
//! and interpreter to evaluate the `CapToken → CapsMask → CpEffect` pipeline.
//! The VM uses eight 32-bit registers and a fixed-size memory region, keeping
//! the design small enough to run on the rendezvous hot path without `std` or
//! heap allocations. The host side maps the emitted [`Action`] values back into
//! the control and data planes.

/// EPF dispatch glue for rendezvous integration.
pub mod dispatch;
/// EPF host interface.
pub mod host;
/// Bytecode image loader.
pub mod loader;
/// Opcode definitions.
pub mod ops;
/// Bytecode verifier.
pub mod verifier;
/// VM execution engine.
pub mod vm;
/// Typed VM context helpers.
pub mod vm_ctx;

pub use dispatch::RaOp;
pub use host::{HostError, HostSlots, Machine};
pub use loader::{ImageLoader, LoaderError};
pub use verifier::{Header, VerifiedImage, VerifyError};
pub use vm::{Slot, Trap, Vm, VmAction, VmCtx};
pub use vm_ctx::{NoObs, VmCtx as TypedVmCtx};

use crate::{
    control::cap::CapsMask,
    observe::TapEvent,
    rendezvous::{Lane, SessionId},
};

/// Abort outcome emitted by the policy VM (or by the host when mapping traps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbortInfo {
    pub reason: u16,
    pub trap: Option<Trap>,
}

/// Unified action surface consumed by slot owners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Proceed,
    Abort(AbortInfo),
    Ra(RaOp),
    Tap { id: u16, arg0: u32, arg1: u32 },
    Route { arm: u8 },
}

const ABORT_UNAUTHORISED_RA: u16 = 0xFF00;

/// Execute the EPF VM installed for `slot` and translate the result into [`Action`].
#[inline]
pub fn run(
    host_slots: &HostSlots<'_>,
    slot: Slot,
    event: &TapEvent,
    caps: CapsMask,
    session: Option<SessionId>,
    lane: Option<Lane>,
) -> Action {
    run_with(host_slots, slot, event, caps, session, lane, |_| {})
}

/// Execute the VM with an opportunity to configure the [`VmCtx`] prior to dispatch.
pub fn run_with<F>(
    host_slots: &HostSlots<'_>,
    slot: Slot,
    event: &TapEvent,
    caps: CapsMask,
    session: Option<SessionId>,
    lane: Option<Lane>,
    configure: F,
) -> Action
where
    F: FnOnce(&mut VmCtx<'_>),
{
    let vm_action = host_slots.execute_with(slot, event, caps, session, lane, configure);
    convert_action(slot, vm_action)
}

fn convert_action(slot: Slot, vm_action: VmAction) -> Action {
    match vm_action {
        VmAction::Proceed => Action::Proceed,
        VmAction::Abort { reason } => Action::Abort(AbortInfo { reason, trap: None }),
        VmAction::Trap(trap) => Action::Abort(AbortInfo {
            reason: trap_reason(trap),
            trap: Some(trap),
        }),
        VmAction::Tap { id, arg0, arg1 } => Action::Tap { id, arg0, arg1 },
        VmAction::Route { arm } => Action::Route { arm },
        VmAction::Ra(op) => {
            if matches!(slot, Slot::Rendezvous) {
                Action::Ra(op)
            } else {
                Action::Abort(AbortInfo {
                    reason: ABORT_UNAUTHORISED_RA,
                    trap: Some(Trap::IllegalSyscall),
                })
            }
        }
    }
}

fn trap_reason(trap: Trap) -> u16 {
    match trap {
        Trap::FuelExhausted => 0x0001,
        Trap::IllegalOpcode(op) => 0x0100 | op as u16,
        Trap::OutOfBounds => 0x0200,
        Trap::IllegalSyscall => 0x0300,
        Trap::VerifyFailed => 0x0400,
    }
}
