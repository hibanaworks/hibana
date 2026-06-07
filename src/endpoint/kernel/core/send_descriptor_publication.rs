use super::{DescriptorPublicationAuthority, SendDescriptorTerminal};

pub(crate) struct SendDescriptorPublication<'rv> {
    publisher: DescriptorPublicationAuthority<'rv>,
    terminal: SendDescriptorTerminal<'rv>,
    _permit: PostKernelDescriptorPermit<'rv>,
}

pub(crate) struct PostKernelDescriptorPermit<'permit> {
    _borrow: core::marker::PhantomData<&'permit mut ()>,
}

impl<'permit> PostKernelDescriptorPermit<'permit> {
    #[inline(always)]
    const fn new() -> Self {
        Self {
            _borrow: core::marker::PhantomData,
        }
    }
}

impl<'rv> SendDescriptorPublication<'rv> {
    pub(in crate::endpoint::kernel::core) const fn none() -> Self {
        Self {
            publisher: DescriptorPublicationAuthority::none(),
            terminal: SendDescriptorTerminal::none(),
            _permit: PostKernelDescriptorPermit::new(),
        }
    }

    pub(in crate::endpoint::kernel::core) fn new(
        publisher: DescriptorPublicationAuthority<'rv>,
        terminal: SendDescriptorTerminal<'rv>,
    ) -> Self {
        if terminal.is_none() {
            return Self::none();
        }
        Self {
            publisher,
            terminal,
            _permit: PostKernelDescriptorPermit::new(),
        }
    }

    pub(crate) fn publish(self) {
        let Self {
            publisher,
            terminal,
            _permit,
        } = self;
        if let Some(ticket) = terminal.into_ticket() {
            publisher.publish(_permit, ticket);
        }
    }
}
