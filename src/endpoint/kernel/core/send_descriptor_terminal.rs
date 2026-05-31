use super::{DescriptorTerminal, Lane, SendCommitPlan};

pub(crate) struct SendDescriptorTerminal<'rv> {
    ticket: DescriptorTerminal,
    _borrow: core::marker::PhantomData<&'rv ()>,
}

impl<'rv> SendDescriptorTerminal<'rv> {
    pub(in crate::endpoint::kernel::core) const fn none() -> Self {
        Self {
            ticket: DescriptorTerminal::none(),
            _borrow: core::marker::PhantomData,
        }
    }

    pub(in crate::endpoint::kernel::core) fn terminal(ticket: DescriptorTerminal) -> Self {
        if ticket.is_none() {
            return Self::none();
        }
        Self {
            ticket,
            _borrow: core::marker::PhantomData,
        }
    }

    pub(crate) fn is_none(&self) -> bool {
        self.ticket.is_none()
    }

    pub(crate) fn into_ticket(self) -> Option<DescriptorTerminal> {
        let Self { ticket, _borrow: _ } = self;
        if ticket.is_none() { None } else { Some(ticket) }
    }
}

pub(crate) struct EndpointRevocationTerminal<'rv> {
    send: Option<SendCommitPlan<'rv>>,
    waiter_lane: Option<Lane>,
}

impl<'rv> EndpointRevocationTerminal<'rv> {
    pub(crate) const fn none() -> Self {
        Self {
            send: None,
            waiter_lane: None,
        }
    }

    pub(in crate::endpoint::kernel) fn set_send_plan(&mut self, plan: SendCommitPlan<'rv>) {
        assert!(self.send.is_none());
        self.send = Some(plan);
    }

    pub(in crate::endpoint::kernel) fn set_waiter_lane(&mut self, lane: Lane) {
        self.waiter_lane = Some(lane);
    }

    pub(crate) const fn waiter_lane(&self) -> Option<Lane> {
        self.waiter_lane
    }

    pub(crate) fn rollback_send_with<R>(&mut self, rollback: &mut R)
    where
        R: EndpointRevocationDescriptorRollback + ?Sized,
    {
        if let Some(plan) = self.send.take() {
            let (control, descriptor) = plan.into_rollback_parts();
            if let Some(ticket) = descriptor.into_ticket() {
                rollback.rollback_endpoint_revocation_descriptor(ticket);
            }
            drop(control);
        }
    }
}

pub(crate) trait EndpointRevocationDescriptorRollback {
    fn rollback_endpoint_revocation_descriptor(&mut self, ticket: DescriptorTerminal);
}
