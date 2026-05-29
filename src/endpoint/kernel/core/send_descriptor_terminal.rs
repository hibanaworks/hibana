use super::{DescriptorTerminal, DescriptorTerminalPublisher};

pub(crate) struct SendDescriptorTerminal<'rv> {
    ticket: DescriptorTerminal,
    _borrow: core::marker::PhantomData<&'rv ()>,
}

impl<'rv> SendDescriptorTerminal<'rv> {
    #[inline]
    pub(in crate::endpoint::kernel::core) const fn none() -> Self {
        Self {
            ticket: DescriptorTerminal::none(),
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::core) fn terminal(ticket: DescriptorTerminal) -> Self {
        if ticket.is_none() {
            Self::none()
        } else {
            Self {
                ticket,
                _borrow: core::marker::PhantomData,
            }
        }
    }

    #[inline(always)]
    pub(crate) fn is_none(&self) -> bool {
        self.ticket.is_none()
    }

    #[inline(always)]
    pub(crate) fn into_ticket(self) -> Option<DescriptorTerminal> {
        let Self { ticket, _borrow: _ } = self;
        if ticket.is_none() {
            drop(ticket);
            None
        } else {
            Some(ticket)
        }
    }
}

pub(crate) struct SendDescriptorPublication<'rv> {
    publisher: DescriptorTerminalPublisher<'rv>,
    terminal: SendDescriptorTerminal<'rv>,
}

impl<'rv> SendDescriptorPublication<'rv> {
    #[inline]
    pub(in crate::endpoint::kernel::core) const fn none() -> Self {
        Self {
            publisher: DescriptorTerminalPublisher::none(),
            terminal: SendDescriptorTerminal::none(),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::core) fn new(
        publisher: DescriptorTerminalPublisher<'rv>,
        terminal: SendDescriptorTerminal<'rv>,
    ) -> Self {
        if terminal.is_none() {
            Self::none()
        } else {
            Self {
                publisher,
                terminal,
            }
        }
    }

    #[inline(always)]
    pub(crate) fn publish(self) {
        let Self {
            publisher,
            terminal,
        } = self;
        if let Some(ticket) = terminal.into_ticket() {
            publisher.publish(ticket);
        }
    }
}
