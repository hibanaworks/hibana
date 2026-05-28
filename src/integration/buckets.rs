pub mod program {
    pub use crate::global::MessageSpec;
    pub use crate::global::role_program::{RoleProgram, project};

    /// Projection-inspection facts for tooling and diagnostics.
    pub mod inspect {
        pub use crate::global::program::{
            Projectable, ProjectionAtomSpec, ProjectionMetadataVisitor, ProjectionPolicySpec,
            ProjectionProgramFacts, ProjectionScopeSpec,
        };
    }
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
    pub use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse, RING_EVENTS};
}

/// Binding and ingress-evidence surface.
pub mod binding {
    pub use crate::binding::{BindingError, BindingSlot, Channel, IngressEvidence};
}

/// Resolver and decision-input surface for dynamic policy.
pub mod policy {
    pub use crate::control::cluster::core::{
        DecisionArm, DecisionResolution, ResolverContext, ResolverError, ResolverRef,
    };

    /// Decision-policy input and attribute metadata.
    pub mod signals {
        pub use crate::transport::context::{PolicyAttrs, PolicyInput, PolicySignals};
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
        CapShot, ControlResourceKind, GenericCapToken, HandleView, ResourceKind,
    };
}

/// Wire payload codec surface.
pub mod wire {
    pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};
}

/// Transport I/O surface plus observation/detail owners.
pub mod transport {
    pub use crate::transport::{FrameLabel, Outgoing, PortOpen, Transport, TransportError};
}
