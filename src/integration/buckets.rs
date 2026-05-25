pub mod program {
    pub use crate::global::MessageSpec;
    pub use crate::global::program::{
        Projectable, ProjectionAtomSpec, ProjectionMetadataVisitor, ProjectionPolicySpec,
        ProjectionProgramFacts, ProjectionScopeSpec,
    };
    #[cfg(any(feature = "std", test))]
    pub use crate::global::program::{ProjectionMessageSpec, ProjectionTypeFingerprint};
    pub use crate::global::role_program::{RoleProgram, project};
}

/// Protocol-neutral identifiers used by integration crates.
pub mod ids {
    pub use crate::control::types::{Lane, RendezvousId, SessionId};
    pub use crate::eff::EffIndex;
}

/// Everyday runtime setup owners for caller-provided storage and clocks.
pub mod runtime {
    pub use crate::observe::core::TapEvent;
    pub use crate::runtime::config::{Clock, Config, CounterClock, RuntimeStorage};
    pub use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse};
}

/// Binding and ingress-evidence surface.
pub mod binding {
    pub use crate::binding::{BindingSlot, NoBinding};

    /// Binding method details for custom demux and channel integration.
    pub mod advanced {
        pub use crate::binding::{Channel, IngressEvidence, TransportOpsError};
    }
}

/// Resolver and slot-input provider surface for dynamic policy.
pub mod policy {
    pub use crate::control::cluster::core::{
        LoopResolution, ResolverContext, ResolverError, ResolverRef, RouteResolution,
    };
    pub use crate::transport::context::PolicySignalsProvider;

    /// Slot-scoped policy input and attribute metadata.
    pub mod signals {
        pub use crate::policy_runtime::PolicySlot;
        pub use crate::transport::context::{ContextId, ContextValue, PolicyAttrs, PolicySignals};

        /// Fixed metadata keys for resolver-context attributes.
        pub mod core {
            pub use crate::transport::context::core::{
                CONGESTION_MARKS, CONGESTION_WINDOW, IN_FLIGHT_BYTES, LANE, LATENCY_US,
                LATEST_ACK_PN, PACING_INTERVAL_US, PTO_COUNT, QUEUE_DEPTH, RETRANSMISSIONS, RV_ID,
                SESSION_ID, SRTT_US, TAG, TRANSPORT_ALGORITHM,
            };
        }
    }
}

/// Canonical capability-token surface plus control-kind owners.
pub mod cap {
    /// Control descriptor and standard control-kind catalogue.
    pub mod control {
        pub use crate::control::cap::mint::{CAP_HANDLE_LEN, CapError, ControlOp, ControlPath};
        pub use crate::control::cap::resource_kinds::{
            LoopBreakKind, LoopContinueKind, RouteDecisionKind,
        };
        pub use crate::global::const_dsl::{ControlScopeKind, ScopeId};
    }

    pub use crate::control::cap::mint::{
        CapShot, ControlResourceKind, GenericCapToken, ResourceKind,
    };
}

/// Wire payload codec surface.
pub mod wire {
    pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};
}

/// Transport I/O surface plus observation/detail owners.
pub mod transport {
    pub use crate::transport::{FrameLabel, Outgoing, PortOpen, Transport, TransportError};
    pub use crate::transport::{
        TransportEvent, TransportEventKind, TransportEventMeta, TransportMetrics,
    };
}
