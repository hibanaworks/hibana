//! Slot-level EPF policy contract.
//!
//! This is the single source of truth for slot non-use rules:
//! - Route / EndpointTx / EndpointRx may consume `GET_INPUT`.
//! - Forward / Rendezvous must not consume `GET_INPUT`.

use super::Slot;

/// Static contract associated with each VM slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotPolicyContract {
    pub(crate) allows_get_input: bool,
    pub(crate) allows_attr: bool,
    pub(crate) allows_mem_ops: bool,
    pub(crate) source: SlotPolicySource,
}

/// Policy signal source associated with a slot contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotPolicySource {
    Binding,
    Zero,
}

impl SlotPolicyContract {
    const fn new(
        allows_get_input: bool,
        allows_attr: bool,
        allows_mem_ops: bool,
        source: SlotPolicySource,
    ) -> Self {
        Self {
            allows_get_input,
            allows_attr,
            allows_mem_ops,
            source,
        }
    }
}

/// Return the policy contract for a slot.
#[inline]
pub(crate) const fn slot_policy_contract(slot: Slot) -> SlotPolicyContract {
    match slot {
        Slot::Route | Slot::EndpointTx | Slot::EndpointRx => SlotPolicyContract::new(
            true,
            true,
            !matches!(slot, Slot::Route),
            SlotPolicySource::Binding,
        ),
        Slot::Forward | Slot::Rendezvous => {
            SlotPolicyContract::new(false, false, true, SlotPolicySource::Zero)
        }
    }
}

/// Whether this slot may use `GET_INPUT`.
#[inline]
#[cfg(test)]
pub(crate) const fn slot_allows_get_input(slot: Slot) -> bool {
    slot_policy_contract(slot).allows_get_input
}

/// Whether this slot may use memory read/write opcodes (`LOAD_MEM`, `STORE_MEM`).
#[inline]
#[cfg(test)]
pub(crate) const fn slot_allows_mem_ops(slot: Slot) -> bool {
    slot_policy_contract(slot).allows_mem_ops
}

/// Default policy input attached to this slot contract.
#[inline]
pub(crate) const fn slot_default_input(slot: Slot) -> [u32; 4] {
    match slot_policy_contract(slot).source {
        SlotPolicySource::Binding => [0; 4],
        SlotPolicySource::Zero => [0; 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_and_endpoint_slots_allow_inputs() {
        assert!(slot_allows_get_input(Slot::Route));
        assert!(slot_allows_get_input(Slot::EndpointTx));
        assert!(slot_allows_get_input(Slot::EndpointRx));
        assert!(!slot_allows_mem_ops(Slot::Route));
        assert!(slot_allows_mem_ops(Slot::EndpointTx));
        assert!(slot_allows_mem_ops(Slot::EndpointRx));
    }

    #[test]
    fn forward_and_rendezvous_forbid_inputs() {
        assert!(!slot_allows_get_input(Slot::Forward));
        assert!(!slot_allows_get_input(Slot::Rendezvous));
        assert!(slot_allows_mem_ops(Slot::Forward));
        assert!(slot_allows_mem_ops(Slot::Rendezvous));
        assert!(!slot_policy_contract(Slot::Forward).allows_attr);
        assert!(!slot_policy_contract(Slot::Rendezvous).allows_attr);
        assert!(matches!(
            slot_policy_contract(Slot::Forward).source,
            SlotPolicySource::Zero
        ));
        assert!(matches!(
            slot_policy_contract(Slot::Rendezvous).source,
            SlotPolicySource::Zero
        ));
    }
}
