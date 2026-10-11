use super::Port;
use crate::{rendezvous::core::EndpointLeaseRecord, transport::Transport};

impl<'r, T: Transport + 'r> Port<'r, T> {
    #[inline]
    pub(crate) fn seal_session_membership(&self) {
        EndpointLeaseRecord::seal_session_membership(self.endpoint_lease_storage, self.sid);
    }
}
