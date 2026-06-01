use super::PreparedDescriptorEffect;
use crate::control::lease::core::RendezvousOwnerProof;

impl<Proof> PreparedDescriptorEffect<Proof> {
    #[inline]
    pub(super) const fn new(owner: RendezvousOwnerProof, proof: Proof) -> Self {
        Self { owner, proof }
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) fn into_parts(
        self,
    ) -> (RendezvousOwnerProof, Proof) {
        (self.owner, self.proof)
    }
}
