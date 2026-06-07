use core::marker::PhantomData;

use super::DescriptorTerminal;
use crate::control::cluster::core::SessionCluster;
use crate::endpoint::kernel::PostKernelDescriptorPermit;

/// Post-kernel authority for consuming a prepared descriptor terminal.
///
/// Endpoint-resident send state carries only `DescriptorTerminal`. This object
/// is minted for the carrier-level publication permit after the endpoint kernel
/// borrow has closed. It must not be used from inside an active `ControlCore`
/// mutation closure; topology revocation rolls descriptor tickets back through
/// its explicit post-core permit instead.
pub(crate) struct DescriptorPublicationAuthority<'cfg> {
    cluster: *const (),
    ops: &'static DescriptorPublicationAuthorityOps,
    _borrow: PhantomData<&'cfg ()>,
}

struct DescriptorPublicationAuthorityOps {
    publish: unsafe fn(*const (), DescriptorTerminal),
}

impl<'cfg> DescriptorPublicationAuthority<'cfg> {
    #[inline]
    pub(crate) const fn none() -> Self {
        unsafe fn ignore_terminal(_cluster: *const (), _ticket: DescriptorTerminal) {}
        static OPS: DescriptorPublicationAuthorityOps = DescriptorPublicationAuthorityOps {
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
            ops: &DescriptorPublicationAuthorityOps {
                publish: publish_impl::<'cfg, T, U, C, MAX_RV>,
            },
            _borrow: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn publish(
        self,
        _permit: PostKernelDescriptorPermit<'_>,
        ticket: DescriptorTerminal,
    ) {
        unsafe {
            // SAFETY: the publication authority was minted from the same
            // cluster owner that built `ticket`; publication consumes the
            // ticket exactly once with the post-kernel permit.
            (self.ops.publish)(self.cluster, ticket);
        }
    }
}
