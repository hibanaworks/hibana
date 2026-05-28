use super::{DescriptorEffect, DescriptorEffectTerminal, Generation, Lane, SessionId};
use crate::control::lease::core::RendezvousOwnerProof;

impl DescriptorEffectTerminal {
    #[inline]
    pub(super) const fn new(
        effect: DescriptorEffect,
        owner: RendezvousOwnerProof,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Self {
        Self {
            effect,
            owner,
            sid,
            lane,
            generation,
        }
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn effect(
        &self,
    ) -> DescriptorEffect {
        self.effect
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn owner(
        &self,
    ) -> RendezvousOwnerProof {
        self.owner
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn sid(
        &self,
    ) -> SessionId {
        self.sid
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn lane(
        &self,
    ) -> Lane {
        self.lane
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn generation(
        &self,
    ) -> Generation {
        self.generation
    }
}
