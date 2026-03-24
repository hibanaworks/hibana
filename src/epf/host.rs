//! Host integration for executing EPF VM policies without global state.

use core::{
    array,
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering},
};

use crate::{
    control::cap::mint::CapsMask,
    control::types::{Lane, SessionId},
    observe::core::TapEvent,
    rendezvous::slots::{SLOT_COUNT, slot_index},
};

use super::{
    PolicyMode,
    verifier::{Header, compute_hash},
    vm::{Slot, Vm, VmAction, VmCtx},
};

/// Errors surfaced by the policy host registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostError {
    SlotOccupied,
    SlotEmpty,
    InvalidFuel,
    ScratchTooSmall { requested: usize, available: usize },
    ScratchTooLarge { provided: usize, max: usize },
}

/// Registered policy machine (bytecode + scratch + fuel budget).
pub(crate) struct Machine<'arena> {
    code: &'arena [u8],
    mem_ptr: *mut u8,
    mem_len: usize,
    fuel_max: u16,
    digest: u32,
    fuel: AtomicU16,
    lock: AtomicBool,
}

unsafe impl Send for Machine<'_> {}
unsafe impl Sync for Machine<'_> {}

impl<'arena> Machine<'arena> {
    /// Construct a new machine with an explicit memory length.
    pub(crate) fn with_mem(
        code: &'arena [u8],
        scratch: &'arena mut [u8],
        mem_len: usize,
        fuel_max: u16,
    ) -> Result<Self, HostError> {
        if fuel_max == 0 {
            return Err(HostError::InvalidFuel);
        }
        if mem_len > scratch.len() {
            return Err(HostError::ScratchTooSmall {
                requested: mem_len,
                available: scratch.len(),
            });
        }
        let max_mem = Header::max_mem_len();
        if mem_len > max_mem {
            return Err(HostError::ScratchTooLarge {
                provided: mem_len,
                max: max_mem,
            });
        }
        Ok(Self {
            code,
            mem_ptr: scratch.as_mut_ptr(),
            mem_len,
            fuel_max,
            digest: compute_hash(code),
            fuel: AtomicU16::new(fuel_max),
            lock: AtomicBool::new(false),
        })
    }

    fn acquire(&self) {
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spin_loop();
        }
    }

    fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }

    #[inline]
    pub(crate) const fn digest(&self) -> u32 {
        self.digest
    }

    fn execute(&self, ctx: &mut VmCtx<'_>) -> VmAction {
        self.acquire();

        let remaining = self.fuel.load(Ordering::Acquire);
        let initial_fuel = if remaining == 0 {
            self.fuel_max
        } else {
            remaining
        };

        let mem_len = self.mem_len;
        // SAFETY: mem_ptr points to scratch memory owned for the lifetime of the machine.
        let mem_slice = unsafe { core::slice::from_raw_parts_mut(self.mem_ptr, mem_len) };
        let mut vm = Vm::new(self.code, mem_slice, initial_fuel);
        let action = vm.execute(ctx);
        self.fuel.store(vm.fuel, Ordering::Release);

        self.release();
        action
    }
}

impl core::fmt::Debug for Machine<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Machine")
            .field("code_len", &self.code.len())
            .field("mem_len", &self.mem_len)
            .field("fuel_max", &self.fuel_max)
            .field("digest", &self.digest)
            .finish()
    }
}

/// In-memory registry of machines backed by a slot arena.
pub(crate) struct HostSlots<'arena> {
    machines: [Option<Machine<'arena>>; SLOT_COUNT],
    policy_modes: [AtomicU8; SLOT_COUNT],
    active_digests: [AtomicU32; SLOT_COUNT],
    last_fuel_used: [AtomicU16; SLOT_COUNT],
}

impl<'arena> Default for HostSlots<'arena> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'arena> HostSlots<'arena> {
    const MODE_SHADOW: u8 = 0;
    const MODE_ENFORCE: u8 = 1;

    #[inline]
    const fn encode_mode(mode: PolicyMode) -> u8 {
        match mode {
            PolicyMode::Shadow => Self::MODE_SHADOW,
            PolicyMode::Enforce => Self::MODE_ENFORCE,
        }
    }

    #[inline]
    const fn decode_mode(raw: u8) -> PolicyMode {
        match raw {
            Self::MODE_SHADOW => PolicyMode::Shadow,
            _ => PolicyMode::Enforce,
        }
    }

    pub(crate) fn new() -> Self {
        Self {
            machines: array::from_fn(|_| None),
            policy_modes: array::from_fn(|_| AtomicU8::new(Self::MODE_ENFORCE)),
            active_digests: array::from_fn(|_| AtomicU32::new(0)),
            last_fuel_used: array::from_fn(|_| AtomicU16::new(0)),
        }
    }

    #[inline]
    fn index(slot: Slot) -> usize {
        slot_index(slot)
    }

    pub(crate) fn install(
        &mut self,
        slot: Slot,
        machine: Machine<'arena>,
    ) -> Result<(), HostError> {
        let idx = Self::index(slot);
        if self.machines[idx].is_some() {
            return Err(HostError::SlotOccupied);
        }
        self.active_digests[idx].store(machine.digest(), Ordering::Release);
        self.machines[idx] = Some(machine);
        Ok(())
    }

    pub(crate) fn uninstall(&mut self, slot: Slot) -> Result<(), HostError> {
        let idx = Self::index(slot);
        if self.machines[idx].is_none() {
            return Err(HostError::SlotEmpty);
        }
        self.machines[idx] = None;
        self.active_digests[idx].store(0, Ordering::Release);
        self.last_fuel_used[idx].store(0, Ordering::Release);
        Ok(())
    }

    #[inline]
    pub(crate) fn set_policy_mode(&self, slot: Slot, mode: PolicyMode) {
        self.policy_modes[Self::index(slot)].store(Self::encode_mode(mode), Ordering::Release);
    }

    #[inline]
    pub(crate) fn policy_mode(&self, slot: Slot) -> PolicyMode {
        let raw = self.policy_modes[Self::index(slot)].load(Ordering::Acquire);
        Self::decode_mode(raw)
    }

    #[inline]
    pub(crate) fn active_digest(&self, slot: Slot) -> u32 {
        self.active_digests[Self::index(slot)].load(Ordering::Acquire)
    }

    #[inline]
    pub(crate) fn last_fuel_used(&self, slot: Slot) -> u16 {
        self.last_fuel_used[Self::index(slot)].load(Ordering::Acquire)
    }

    pub(crate) fn execute_with<F>(
        &self,
        slot: Slot,
        event: &TapEvent,
        caps: CapsMask,
        session: Option<SessionId>,
        lane: Option<Lane>,
        configure: F,
    ) -> VmAction
    where
        F: FnOnce(&mut VmCtx<'_>),
    {
        let mut ctx = VmCtx::new(slot, event, caps);
        if let Some(session) = session {
            ctx.set_session(session);
        }
        if let Some(lane) = lane {
            ctx.set_lane(lane);
        }
        configure(&mut ctx);

        match &self.machines[Self::index(slot)] {
            Some(machine) => {
                let remaining_before = machine.fuel.load(Ordering::Acquire);
                let initial_fuel = if remaining_before == 0 {
                    machine.fuel_max
                } else {
                    remaining_before
                };
                let action = machine.execute(&mut ctx);
                let remaining_after = machine.fuel.load(Ordering::Acquire);
                let used = initial_fuel.saturating_sub(remaining_after);
                self.last_fuel_used[Self::index(slot)].store(used, Ordering::Release);
                action
            }
            None => {
                self.last_fuel_used[Self::index(slot)].store(0, Ordering::Release);
                VmAction::Proceed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{epf::ops, observe::core::TapEvent};

    use std::boxed::Box;
    use std::vec;

    fn setup_machine(code: &'static [u8]) -> Machine<'static> {
        let scratch = Box::leak(vec![0u8; 64].into_boxed_slice());
        Machine::with_mem(code, scratch, scratch.len(), 8).expect("machine")
    }

    #[test]
    fn host_returns_effect_directive() {
        static CODE: [u8; 3] = [ops::instr::ACT_EFFECT, ops::effect::CHECKPOINT, 0x00];
        let machine = setup_machine(&CODE);
        let mut slots = HostSlots::new();
        slots.install(Slot::Rendezvous, machine).expect("install");

        static EVENT: TapEvent = crate::observe::events::RawEvent::zero();
        let result = slots.execute_with(
            Slot::Rendezvous,
            &EVENT,
            CapsMask::allow_all(),
            Some(SessionId::new(7)),
            Some(Lane::new(3)),
            |_| {},
        );

        assert!(matches!(result, VmAction::Ra(_)));
        slots.uninstall(Slot::Rendezvous).expect("uninstall");
    }

    #[test]
    fn shadow_mode_suppresses_enforcement() {
        static CODE: [u8; 3] = [ops::instr::ACT_ABORT, 0x34, 0x12];
        let machine = setup_machine(&CODE);
        let mut slots = HostSlots::new();
        slots.install(Slot::Route, machine).expect("install");

        static EVENT: TapEvent = crate::observe::events::RawEvent::zero();
        let enforce = crate::epf::run_with(
            &slots,
            Slot::Route,
            &EVENT,
            CapsMask::allow_all(),
            None,
            None,
            |_| {},
        );
        assert!(matches!(
            enforce,
            crate::epf::Action::Abort(crate::epf::AbortInfo { reason: 0x1234, .. })
        ));

        slots.set_policy_mode(Slot::Route, PolicyMode::Shadow);
        let shadow = crate::epf::run_with(
            &slots,
            Slot::Route,
            &EVENT,
            CapsMask::allow_all(),
            None,
            None,
            |_| {},
        );
        assert_eq!(shadow, crate::epf::Action::Proceed);
    }
}
