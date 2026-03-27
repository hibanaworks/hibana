//! Protocol-neutral substrate surface for protocol implementors.

pub use crate::control::cluster::error::{AttachError, CpError};

pub use crate::control::types::{Lane, RendezvousId, SessionId};
pub use crate::eff::EffIndex;
pub use crate::transport::Transport;

/// Protocol-neutral session cluster facade for protocol implementors.
#[repr(transparent)]
pub struct SessionCluster<'cfg, T, U, C, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    inner: crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline]
    pub fn new(clock: &'cfg C) -> Self {
        Self {
            inner: crate::control::cluster::core::SessionCluster::new(clock),
        }
    }

    #[inline]
    pub fn add_rendezvous_from_config(
        &self,
        config: crate::substrate::runtime::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousId, CpError> {
        self.inner.add_rendezvous_from_config(config, transport)
    }

    #[inline]
    pub fn enter<'prog, const ROLE: u8, Steps, Mint, B>(
        &'cfg self,
        rv: RendezvousId,
        sid: SessionId,
        program: &'prog crate::g::advanced::RoleProgram<'prog, ROLE, Steps, Mint>,
        binding: B,
    ) -> Result<
        crate::Endpoint<
            'cfg,
            ROLE,
            T,
            U,
            C,
            crate::substrate::cap::advanced::EpochTbl,
            MAX_RV,
            Mint,
            B,
        >,
        AttachError,
    >
    where
        B: crate::substrate::binding::BindingSlot,
        Mint: crate::substrate::cap::advanced::MintConfigMarker,
    {
        self.inner.enter(rv, sid, program, binding)
    }

    #[inline]
    pub fn set_resolver<'prog, const POLICY: u16, const ROLE: u8, Steps, Mint>(
        &self,
        rv: RendezvousId,
        program: &'prog crate::g::advanced::RoleProgram<'prog, ROLE, Steps, Mint>,
        resolver: crate::substrate::policy::ResolverRef<'cfg>,
    ) -> Result<(), CpError>
    where
        Mint: crate::substrate::cap::advanced::MintConfigMarker,
    {
        self.inner
            .set_resolver::<POLICY, ROLE, Steps, Mint>(rv, program, resolver)
    }

    #[inline]
    pub(crate) fn as_kernel(
        &self,
    ) -> &crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV> {
        &self.inner
    }
}

pub mod runtime {
    pub use crate::runtime::config::{Clock, Config, CounterClock};
    pub use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse};
}

pub mod mgmt {
    pub use crate::runtime::mgmt::{
        LoadBegin, LoadChunk, LoadReport, MgmtError, Reply, StatsResp, SubscribeReq,
        TransitionReport,
    };

    pub mod session {
        pub mod tap {
            pub use crate::observe::core::TapEvent;
        }

        pub use crate::runtime::mgmt::{LoadRequest, Request, SlotRequest};

        pub fn enter_controller<'cfg, T, U, C, B, const MAX_RV: usize>(
            cluster: &'cfg crate::substrate::SessionCluster<'cfg, T, U, C, MAX_RV>,
            rv_id: crate::substrate::RendezvousId,
            sid: crate::substrate::SessionId,
            binding: B,
        ) -> Result<
            crate::Endpoint<
                'cfg,
                0,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                crate::substrate::cap::advanced::MintConfig,
                B,
            >,
            crate::substrate::AttachError,
        >
        where
            T: crate::substrate::Transport + 'cfg,
            U: crate::substrate::runtime::LabelUniverse + 'cfg,
            C: crate::substrate::runtime::Clock + 'cfg,
            B: crate::substrate::binding::BindingSlot,
        {
            crate::runtime::mgmt::enter_controller(cluster.as_kernel(), rv_id, sid, binding)
        }

        pub fn enter_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(
            cluster: &'cfg crate::substrate::SessionCluster<'cfg, T, U, C, MAX_RV>,
            rv_id: crate::substrate::RendezvousId,
            sid: crate::substrate::SessionId,
            binding: B,
        ) -> Result<
            crate::Endpoint<
                'cfg,
                1,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                crate::substrate::cap::advanced::MintConfig,
                B,
            >,
            crate::substrate::AttachError,
        >
        where
            T: crate::substrate::Transport + 'cfg,
            U: crate::substrate::runtime::LabelUniverse + 'cfg,
            C: crate::substrate::runtime::Clock + 'cfg,
            B: crate::substrate::binding::BindingSlot,
        {
            crate::runtime::mgmt::enter_cluster(cluster.as_kernel(), rv_id, sid, binding)
        }

        pub fn enter_stream_controller<'cfg, T, U, C, B, const MAX_RV: usize>(
            cluster: &'cfg crate::substrate::SessionCluster<'cfg, T, U, C, MAX_RV>,
            rv_id: crate::substrate::RendezvousId,
            sid: crate::substrate::SessionId,
            binding: B,
        ) -> Result<
            crate::Endpoint<
                'cfg,
                0,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                crate::substrate::cap::advanced::MintConfig,
                B,
            >,
            crate::substrate::AttachError,
        >
        where
            T: crate::substrate::Transport + 'cfg,
            U: crate::substrate::runtime::LabelUniverse + 'cfg,
            C: crate::substrate::runtime::Clock + 'cfg,
            B: crate::substrate::binding::BindingSlot,
        {
            crate::runtime::mgmt::enter_stream_controller(cluster.as_kernel(), rv_id, sid, binding)
        }

        pub fn enter_stream_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(
            cluster: &'cfg crate::substrate::SessionCluster<'cfg, T, U, C, MAX_RV>,
            rv_id: crate::substrate::RendezvousId,
            sid: crate::substrate::SessionId,
            binding: B,
        ) -> Result<
            crate::Endpoint<
                'cfg,
                1,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                crate::substrate::cap::advanced::MintConfig,
                B,
            >,
            crate::substrate::AttachError,
        >
        where
            T: crate::substrate::Transport + 'cfg,
            U: crate::substrate::runtime::LabelUniverse + 'cfg,
            C: crate::substrate::runtime::Clock + 'cfg,
            B: crate::substrate::binding::BindingSlot,
        {
            crate::runtime::mgmt::enter_stream_cluster(cluster.as_kernel(), rv_id, sid, binding)
        }

        impl<'request> Request<'request> {
            pub async fn drive_controller<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
                self,
                endpoint: crate::Endpoint<
                    'lease,
                    0,
                    T,
                    U,
                    C,
                    crate::substrate::cap::advanced::EpochTbl,
                    MAX_RV,
                    Mint,
                    B,
                >,
            ) -> Result<
                (
                    crate::Endpoint<
                        'lease,
                        0,
                        T,
                        U,
                        C,
                        crate::substrate::cap::advanced::EpochTbl,
                        MAX_RV,
                        Mint,
                        B,
                    >,
                    crate::substrate::mgmt::Reply,
                ),
                crate::substrate::mgmt::MgmtError,
            >
            where
                T: crate::substrate::Transport + 'lease,
                U: crate::substrate::runtime::LabelUniverse,
                C: crate::substrate::runtime::Clock,
                Mint: crate::substrate::cap::advanced::MintConfigMarker,
                Mint::Policy: crate::substrate::cap::advanced::AllowsCanonical,
                B: crate::substrate::binding::BindingSlot,
            {
                crate::runtime::mgmt::drive_controller(endpoint, self).await
            }
        }

        pub async fn drive_cluster<'lease, 'cfg, T, U, C, Mint, B, const MAX_RV: usize>(
            cluster: &'lease crate::substrate::SessionCluster<'cfg, T, U, C, MAX_RV>,
            rv_id: crate::substrate::RendezvousId,
            sid: crate::substrate::SessionId,
            endpoint: crate::Endpoint<
                'lease,
                1,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                Mint,
                B,
            >,
        ) -> Result<
            crate::Endpoint<
                'lease,
                1,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                Mint,
                B,
            >,
            crate::substrate::mgmt::MgmtError,
        >
        where
            T: crate::substrate::Transport + 'cfg,
            U: crate::substrate::runtime::LabelUniverse,
            C: crate::substrate::runtime::Clock,
            Mint: crate::substrate::cap::advanced::MintConfigMarker,
            Mint::Policy: crate::substrate::cap::advanced::AllowsCanonical,
            B: crate::substrate::binding::BindingSlot,
        {
            crate::runtime::mgmt::drive_cluster(cluster.as_kernel(), rv_id, sid, endpoint).await
        }

        pub async fn drive_stream_cluster<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(
            endpoint: crate::Endpoint<
                'lease,
                1,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                Mint,
                B,
            >,
            should_continue: F,
        ) -> Result<
            crate::Endpoint<
                'lease,
                1,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                Mint,
                B,
            >,
            crate::substrate::mgmt::MgmtError,
        >
        where
            T: crate::substrate::Transport + 'lease,
            U: crate::substrate::runtime::LabelUniverse,
            C: crate::substrate::runtime::Clock,
            Mint: crate::substrate::cap::advanced::MintConfigMarker,
            Mint::Policy: crate::substrate::cap::advanced::AllowsCanonical,
            F: FnMut() -> bool,
            B: crate::substrate::binding::BindingSlot,
        {
            crate::runtime::mgmt::drive_stream_cluster(endpoint, should_continue).await
        }

        pub async fn drive_stream_controller<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(
            endpoint: crate::Endpoint<
                'lease,
                0,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                Mint,
                B,
            >,
            subscribe: crate::substrate::mgmt::SubscribeReq,
            on_event: F,
        ) -> Result<
            crate::Endpoint<
                'lease,
                0,
                T,
                U,
                C,
                crate::substrate::cap::advanced::EpochTbl,
                MAX_RV,
                Mint,
                B,
            >,
            crate::substrate::mgmt::MgmtError,
        >
        where
            T: crate::substrate::Transport + 'lease,
            U: crate::substrate::runtime::LabelUniverse,
            C: crate::substrate::runtime::Clock,
            Mint: crate::substrate::cap::advanced::MintConfigMarker,
            F: FnMut(crate::substrate::mgmt::session::tap::TapEvent) -> bool,
            B: crate::substrate::binding::BindingSlot,
        {
            crate::runtime::mgmt::drive_stream_controller(endpoint, subscribe, on_event).await
        }
    }
}

pub mod binding {
    pub use crate::binding::{
        BindingSlot, Channel, ChannelDirection, ChannelKey, ChannelStore, IncomingClassification,
        NoBinding, TransportOpsError,
    };
}

pub mod policy {
    pub use crate::control::cluster::core::{
        DynamicResolution, ResolverContext, ResolverError, ResolverRef,
    };
    pub use crate::transport::context::{
        ContextId, ContextValue, PolicyAttrs, PolicySignals, PolicySignalsProvider,
    };

    pub mod core {
        pub use crate::transport::context::core::{
            CONGESTION_MARKS, CONGESTION_WINDOW, IN_FLIGHT_BYTES, LANE, LATENCY_US, LATEST_ACK_PN,
            PACING_INTERVAL_US, PTO_COUNT, QUEUE_DEPTH, RETRANSMISSIONS, RV_ID, SESSION_ID,
            SRTT_US, TAG, TRANSPORT_ALGORITHM,
        };
    }

    pub mod epf {
        pub use crate::epf::verifier::Header;
        pub use crate::epf::vm::Slot;
    }
}

pub mod cap {
    pub mod advanced {
        pub use crate::control::cap::mint::{
            AllowsCanonical, CAP_HANDLE_LEN, CapError, CapsMask, ControlMint, EpochTbl, MintConfig,
            MintConfigMarker, SessionScopedKind,
        };
        pub use crate::control::cap::resource_kinds::{
            CancelAckKind, CancelKind, CheckpointKind, CommitKind, LoadBeginKind, LoadCommitKind,
            LoopBreakKind, LoopContinueKind, LoopDecisionHandle, PolicyActivateKind,
            PolicyAnnotateKind, PolicyLoadKind, PolicyRevertKind, RerouteKind, RollbackKind,
            RouteDecisionHandle, RouteDecisionKind, SpliceAckKind, SpliceIntentKind,
        };
        pub use crate::global::ControlHandling;
        pub use crate::global::const_dsl::{ControlScopeKind, ScopeId};
    }

    pub use crate::control::cap::mint::{
        CapShot, ControlResourceKind, GenericCapToken, ResourceKind,
    };
    pub use crate::control::types::{Many, One};
}

pub mod wire {
    pub use crate::transport::wire::{CodecError, Payload, WireDecode, WireEncode};
}

pub mod transport {
    pub use crate::transport::{
        LocalDirection, Outgoing, SendMeta, TransportAlgorithm, TransportError, TransportEvent,
        TransportEventKind, TransportMetrics, TransportSnapshot,
    };
}
