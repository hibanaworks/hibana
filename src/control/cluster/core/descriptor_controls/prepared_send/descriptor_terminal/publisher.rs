use core::marker::PhantomData;

use super::DescriptorTerminal;
use crate::control::cluster::core::SessionCluster;

#[derive(Clone, Copy)]
pub(crate) struct DescriptorTerminalPublisher<'cfg> {
    cluster: *const (),
    ops: &'static DescriptorTerminalPublisherOps,
    _borrow: PhantomData<&'cfg ()>,
}

struct DescriptorTerminalPublisherOps {
    publish: unsafe fn(*const (), DescriptorTerminal),
}

impl<'cfg> DescriptorTerminalPublisher<'cfg> {
    #[inline]
    pub(crate) const fn none() -> Self {
        unsafe fn ignore_terminal(_cluster: *const (), _ticket: DescriptorTerminal) {}
        static OPS: DescriptorTerminalPublisherOps = DescriptorTerminalPublisherOps {
            publish: ignore_terminal,
        };

        Self {
            cluster: core::ptr::null(),
            ops: &OPS,
            _borrow: PhantomData,
        }
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) fn new<
        T,
        U,
        C,
        const MAX_RV: usize,
    >(
        cluster: &'cfg SessionCluster<'cfg, T, U, C, MAX_RV>,
    ) -> Self
    where
        T: crate::transport::Transport + 'cfg,
        U: crate::runtime::consts::LabelUniverse + 'cfg,
        C: crate::runtime::config::Clock + 'cfg,
    {
        unsafe fn publish_impl<'cfg, T, U, C, const MAX_RV: usize>(
            cluster: *const (),
            ticket: DescriptorTerminal,
        ) where
            T: crate::transport::Transport + 'cfg,
            U: crate::runtime::consts::LabelUniverse + 'cfg,
            C: crate::runtime::config::Clock + 'cfg,
        {
            let cluster = unsafe {
                // SAFETY: `cluster` was captured from the resident SessionCluster
                // owner with the same concrete transport/runtime types.
                &*cluster.cast::<SessionCluster<'cfg, T, U, C, MAX_RV>>()
            };
            cluster.publish_descriptor_terminal(ticket);
        }

        Self {
            cluster: core::ptr::from_ref(cluster).cast(),
            ops: &DescriptorTerminalPublisherOps {
                publish: publish_impl::<'cfg, T, U, C, MAX_RV>,
            },
            _borrow: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn publish(self, ticket: DescriptorTerminal) {
        unsafe {
            // SAFETY: the publisher proof was minted from the same cluster owner
            // that built `ticket`; publication consumes the ticket exactly once.
            (self.ops.publish)(self.cluster, ticket);
        }
    }
}
