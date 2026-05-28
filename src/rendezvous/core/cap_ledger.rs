use super::{CapReleaseCtx, Clock, LabelUniverse, Lane, NonceSeed, Rendezvous, Transport};
impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn next_nonce_seed(&self) -> NonceSeed {
        let ordinal = self.cap_nonce.get();
        let next = ordinal
            .checked_add(1)
            .expect("capability nonce counter exhausted");
        self.cap_nonce.set(next);
        NonceSeed::counter(ordinal)
    }

    #[inline]
    pub(crate) fn next_cap_revision(&self) -> u64 {
        let next = self
            .cap_revision
            .get()
            .checked_add(1)
            .expect("capability revision counter exhausted");
        self.cap_revision.set(next);
        next
    }

    #[inline]
    pub(crate) fn cap_release_ctx(&self, lane: Lane) -> CapReleaseCtx<'_> {
        CapReleaseCtx::new(&self.caps, &self.state_snapshots, &self.cap_revision, lane)
    }
}
