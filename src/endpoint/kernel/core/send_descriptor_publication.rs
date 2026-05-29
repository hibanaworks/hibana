use super::{DescriptorPublicationAuthority, SendDescriptorTerminal};

pub(crate) struct SendDescriptorPublication<'rv> {
    publisher: DescriptorPublicationAuthority<'rv>,
    terminal: SendDescriptorTerminal<'rv>,
    _phase: PostKernelDescriptorPhase<'rv>,
}

pub(crate) struct PostKernelDescriptorPhase<'phase> {
    _borrow: core::marker::PhantomData<&'phase mut ()>,
}

impl<'phase> PostKernelDescriptorPhase<'phase> {
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
            _phase: PostKernelDescriptorPhase::new(),
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
            _phase: PostKernelDescriptorPhase::new(),
        }
    }

    pub(crate) fn publish(self) {
        let Self {
            publisher,
            terminal,
            _phase,
        } = self;
        if let Some(ticket) = terminal.into_ticket() {
            publisher.publish(_phase, ticket);
        }
    }
}
