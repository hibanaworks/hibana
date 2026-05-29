use super::{DescriptorTerminal, Lane};

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
    descriptor: SendDescriptorTerminal<'rv>,
    waiter_lane: Option<Lane>,
}

impl<'rv> EndpointRevocationTerminal<'rv> {
    pub(crate) const fn none() -> Self {
        Self {
            descriptor: SendDescriptorTerminal::none(),
            waiter_lane: None,
        }
    }

    pub(in crate::endpoint::kernel) fn set_descriptor(
        &mut self,
        descriptor: SendDescriptorTerminal<'rv>,
    ) {
        if descriptor.is_none() {
            return;
        }
        assert!(self.descriptor.is_none());
        self.descriptor = descriptor;
    }

    pub(in crate::endpoint::kernel) fn set_waiter_lane(&mut self, lane: Lane) {
        self.waiter_lane = Some(lane);
    }

    pub(crate) const fn waiter_lane(&self) -> Option<Lane> {
        self.waiter_lane
    }

    pub(crate) fn take_descriptor_ticket(&mut self) -> Option<DescriptorTerminal> {
        let descriptor = core::mem::replace(&mut self.descriptor, SendDescriptorTerminal::none());
        descriptor.into_ticket()
    }
}
