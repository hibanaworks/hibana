//! Decision-policy replay input.
//!
//! Resolver input is owned by resolver state. This module keeps the compact
//! replay input word used by policy audit events without exposing numeric slots
//! or extension identifiers to protocol integrations.

/// Internal decision-policy replay input word.
///
/// Protocol-specific resolver input is captured through `ResolverRef` state.
/// This word exists only to keep replay/audit records structurally stable.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PolicyInput {
    primary: u32,
}

impl PolicyInput {
    pub(crate) const ZERO: Self = Self { primary: 0 };

    #[inline]
    #[cfg(test)]
    pub(crate) const fn from_primary(primary: u32) -> Self {
        Self { primary }
    }

    #[inline]
    pub(crate) const fn replay_words(self) -> [u32; 4] {
        [self.primary, 0, 0, 0]
    }

    #[inline]
    pub(crate) const fn primary(self) -> u32 {
        self.primary
    }
}

/// Internal policy replay signal bundle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PolicySignals {
    input: PolicyInput,
}

impl PolicySignals {
    pub(crate) const ZERO: Self = Self::new(PolicyInput::ZERO);

    #[inline]
    pub(crate) const fn new(input: PolicyInput) -> Self {
        Self { input }
    }

    #[inline]
    pub(crate) const fn input(&self) -> PolicyInput {
        self.input
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_signals_zero_defaults() {
        let signals = PolicySignals::ZERO;
        assert_eq!(signals.input(), PolicyInput::ZERO);
    }

    #[test]
    fn policy_signals_carry_input() {
        let route = PolicySignals::new(PolicyInput::from_primary(1));
        let tx = PolicySignals::new(PolicyInput::from_primary(2));
        assert_eq!(route.input().primary(), 1);
        assert_eq!(tx.input().primary(), 2);
    }
}
