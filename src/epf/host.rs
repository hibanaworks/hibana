//! Host integration shim for executing EPF VM policies without global state.

use core::{
    array,
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicU16, Ordering},
};

use crate::{
    control::cap::CapsMask,
    observe::TapEvent,
    rendezvous::{Lane, SessionId},
    rendezvous::{SLOT_COUNT, slot_index},
};

use super::{
    verifier::Header,
    vm::{Slot, Vm, VmAction, VmCtx},
};

/// Errors surfaced by the policy host registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostError {
    SlotOccupied,
    SlotEmpty,
    InvalidFuel,
    ScratchTooSmall { requested: usize, available: usize },
    ScratchTooLarge { provided: usize, max: usize },
}

/// Registered policy machine (bytecode + scratch + fuel budget).
pub struct Machine<'arena> {
    code: &'arena [u8],
    mem_ptr: *mut u8,
    mem_len: usize,
    fuel_max: u16,
    fuel: AtomicU16,
    lock: AtomicBool,
}

unsafe impl Send for Machine<'_> {}
unsafe impl Sync for Machine<'_> {}

impl<'arena> Machine<'arena> {
    /// Construct a new machine using the entire scratch region as VM memory.
    #[inline]
    pub fn new(
        code: &'arena [u8],
        scratch: &'arena mut [u8],
        fuel_max: u16,
    ) -> Result<Self, HostError> {
        Self::with_mem(code, scratch, scratch.len(), fuel_max)
    }

    /// Construct a new machine with an explicit memory length.
    pub fn with_mem(
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
            .finish()
    }
}

/// In-memory registry of machines backed by a slot arena.
pub struct HostSlots<'arena> {
    machines: [Option<Machine<'arena>>; SLOT_COUNT],
}

impl<'arena> Default for HostSlots<'arena> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'arena> HostSlots<'arena> {
    pub fn new() -> Self {
        Self {
            machines: array::from_fn(|_| None),
        }
    }

    #[inline]
    fn index(slot: Slot) -> usize {
        slot_index(slot)
    }

    pub fn install(&mut self, slot: Slot, machine: Machine<'arena>) -> Result<(), HostError> {
        let idx = Self::index(slot);
        if self.machines[idx].is_some() {
            return Err(HostError::SlotOccupied);
        }
        self.machines[idx] = Some(machine);
        Ok(())
    }

    pub fn uninstall(&mut self, slot: Slot) -> Result<(), HostError> {
        let idx = Self::index(slot);
        if self.machines[idx].is_none() {
            return Err(HostError::SlotEmpty);
        }
        self.machines[idx] = None;
        Ok(())
    }

    pub fn execute_with<F>(
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
            Some(machine) => machine.execute(&mut ctx),
            None => VmAction::Proceed,
        }
    }

    #[inline]
    pub fn is_installed(&self, slot: Slot) -> bool {
        self.machines[Self::index(slot)].is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{epf::ops, observe::TapEvent};

    use std::boxed::Box;
    use std::vec;

    fn setup_machine(code: &'static [u8]) -> Machine<'static> {
        let scratch = Box::leak(vec![0u8; 64].into_boxed_slice());
        Machine::new(code, scratch, 8).expect("machine")
    }

    #[test]
    fn host_returns_effect_directive() {
        static CODE: [u8; 3] = [ops::instr::ACT_EFFECT, ops::effect::CHECKPOINT, 0x00];
        let machine = setup_machine(&CODE);
        let mut slots = HostSlots::new();
        slots.install(Slot::Rendezvous, machine).expect("install");

        static EVENT: TapEvent = crate::observe::RawEvent::zero();
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
}
