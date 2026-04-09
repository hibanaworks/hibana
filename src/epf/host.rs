//! Host integration for executing EPF VM policies without global state.

#[cfg(not(test))]
use core::marker::PhantomData;
#[cfg(test)]
use core::{array, cell::Cell};

#[cfg(test)]
use crate::rendezvous::slots::{SLOT_COUNT, slot_index};
#[cfg(test)]
use crate::{
    control::cap::mint::CapsMask,
    control::types::{Lane, SessionId},
    observe::core::TapEvent,
};

#[cfg(test)]
use super::verifier::{Header, compute_hash};
#[cfg(test)]
use super::vm::Vm;
#[cfg(test)]
use super::{
    PolicyMode,
    vm::{Slot, VmAction, VmCtx},
};

/// Errors surfaced by the policy host registry.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostError {
    SlotOccupied,
    SlotEmpty,
    InvalidFuel,
    ScratchTooSmall { requested: usize, available: usize },
    ScratchTooLarge { provided: usize, max: usize },
}

/// Registered policy machine (bytecode + scratch + fuel budget).
#[cfg(test)]
pub(crate) struct Machine<'arena> {
    code: &'arena [u8],
    mem_ptr: *mut u8,
    mem_len: usize,
    fuel_max: u16,
    #[cfg(test)]
    digest: u32,
    fuel: Cell<u16>,
}

#[cfg(test)]
impl<'arena> Machine<'arena> {
    /// Construct a new machine with an explicit memory length.
    #[cfg(test)]
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
            fuel: Cell::new(fuel_max),
        })
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn digest(&self) -> u32 {
        self.digest
    }

    fn execute(&self, ctx: &mut VmCtx<'_>) -> VmAction {
        let remaining = self.fuel.get();
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
        self.fuel.set(vm.fuel);
        action
    }
}

#[cfg(test)]
impl core::fmt::Debug for Machine<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut debug = f.debug_struct("Machine");
        debug.field("code_len", &self.code.len());
        debug.field("mem_len", &self.mem_len);
        debug.field("fuel_max", &self.fuel_max);
        #[cfg(test)]
        debug.field("digest", &self.digest);
        debug.finish()
    }
}

/// In-memory EPF slot registry.
///
/// Production builds keep only the lifetime witness because the current
/// management/install path is test-only. Test builds retain the full slot-backed
/// machine registry and audit state.
pub(crate) struct HostSlots<'arena> {
    #[cfg(test)]
    machines: [Option<Machine<'arena>>; SLOT_COUNT],
    #[cfg(test)]
    policy_modes: [Cell<u8>; SLOT_COUNT],
    #[cfg(test)]
    active_digests: [Cell<u32>; SLOT_COUNT],
    #[cfg(test)]
    last_fuel_used: [Cell<u16>; SLOT_COUNT],
    #[cfg(not(test))]
    _arena: PhantomData<&'arena ()>,
}

impl<'arena> Default for HostSlots<'arena> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'arena> HostSlots<'arena> {
    #[cfg(test)]
    const MODE_SHADOW: u8 = 0;
    #[cfg(test)]
    const MODE_ENFORCE: u8 = 1;

    #[inline]
    #[cfg(test)]
    const fn encode_mode(mode: PolicyMode) -> u8 {
        match mode {
            PolicyMode::Shadow => Self::MODE_SHADOW,
            PolicyMode::Enforce => Self::MODE_ENFORCE,
        }
    }

    #[inline]
    #[cfg(test)]
    const fn decode_mode(raw: u8) -> PolicyMode {
        match raw {
            Self::MODE_SHADOW => PolicyMode::Shadow,
            _ => PolicyMode::Enforce,
        }
    }

    pub(crate) fn new() -> Self {
        #[cfg(test)]
        {
            Self {
                machines: array::from_fn(|_| None),
                policy_modes: array::from_fn(|_| Cell::new(Self::MODE_ENFORCE)),
                active_digests: array::from_fn(|_| Cell::new(0)),
                last_fuel_used: array::from_fn(|_| Cell::new(0)),
            }
        }
        #[cfg(not(test))]
        {
            Self {
                _arena: PhantomData,
            }
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            #[cfg(test)]
            {
                let machines_ptr =
                    core::ptr::addr_of_mut!((*dst).machines).cast::<Option<Machine<'arena>>>();
                let policy_modes_ptr =
                    core::ptr::addr_of_mut!((*dst).policy_modes).cast::<Cell<u8>>();
                let active_digests_ptr =
                    core::ptr::addr_of_mut!((*dst).active_digests).cast::<Cell<u32>>();
                let last_fuel_used_ptr =
                    core::ptr::addr_of_mut!((*dst).last_fuel_used).cast::<Cell<u16>>();
                let mut idx = 0usize;
                while idx < SLOT_COUNT {
                    machines_ptr.add(idx).write(None);
                    policy_modes_ptr
                        .add(idx)
                        .write(Cell::new(Self::MODE_ENFORCE));
                    active_digests_ptr.add(idx).write(Cell::new(0));
                    last_fuel_used_ptr.add(idx).write(Cell::new(0));
                    idx += 1;
                }
            }
            #[cfg(not(test))]
            {
                core::ptr::addr_of_mut!((*dst)._arena).write(PhantomData);
            }
        }
    }

    #[inline]
    #[cfg(test)]
    fn index(slot: Slot) -> usize {
        slot_index(slot)
    }

    #[cfg(test)]
    pub(crate) fn install(
        &mut self,
        slot: Slot,
        machine: Machine<'arena>,
    ) -> Result<(), HostError> {
        let idx = Self::index(slot);
        if self.machines[idx].is_some() {
            return Err(HostError::SlotOccupied);
        }
        self.active_digests[idx].set(machine.digest());
        self.machines[idx] = Some(machine);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn uninstall(&mut self, slot: Slot) -> Result<(), HostError> {
        let idx = Self::index(slot);
        if self.machines[idx].is_none() {
            return Err(HostError::SlotEmpty);
        }
        self.machines[idx] = None;
        self.active_digests[idx].set(0);
        self.last_fuel_used[idx].set(0);
        Ok(())
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn set_policy_mode(&self, slot: Slot, mode: PolicyMode) {
        self.policy_modes[Self::index(slot)].set(Self::encode_mode(mode));
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn policy_mode(&self, slot: Slot) -> PolicyMode {
        let raw = self.policy_modes[Self::index(slot)].get();
        Self::decode_mode(raw)
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn active_digest(&self, slot: Slot) -> u32 {
        self.active_digests[Self::index(slot)].get()
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn last_fuel_used(&self, slot: Slot) -> u16 {
        self.last_fuel_used[Self::index(slot)].get()
    }

    #[cfg(test)]
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
                let remaining_before = machine.fuel.get();
                let initial_fuel = if remaining_before == 0 {
                    machine.fuel_max
                } else {
                    remaining_before
                };
                let action = machine.execute(&mut ctx);
                let remaining_after = machine.fuel.get();
                let used = initial_fuel.saturating_sub(remaining_after);
                self.last_fuel_used[Self::index(slot)].set(used);
                action
            }
            None => {
                self.last_fuel_used[Self::index(slot)].set(0);
                VmAction::Proceed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{epf::ops, observe::core::TapEvent};

    fn setup_machine<'a>(code: &'a [u8], scratch: &'a mut [u8; 64]) -> Machine<'a> {
        let len = scratch.len();
        Machine::with_mem(code, scratch, len, 8).expect("machine")
    }

    #[test]
    fn host_returns_effect_directive() {
        static CODE: [u8; 3] = [ops::instr::ACT_EFFECT, ops::effect::CHECKPOINT, 0x00];
        let mut scratch = [0u8; 64];
        let machine = setup_machine(&CODE, &mut scratch);
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
        let mut scratch = [0u8; 64];
        let machine = setup_machine(&CODE, &mut scratch);
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
