pub mod program {
    pub use crate::global::program::Projectable;
    pub use crate::global::role_program::{RoleProgram, project};
}

/// Protocol-neutral identifiers used by integration crates.
pub mod ids {
    pub use crate::control::types::{Lane, SessionId};
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
    pub use crate::binding::{BindingError, Channel, EndpointSlot, IngressEvidence};
}

/// Resolver and decision-input surface for dynamic policy.
pub mod policy {
    pub use crate::control::cluster::core::{
        DecisionArm, DecisionResolution, ResolverError, ResolverRef,
    };
}

/// Canonical capability-token surface plus control-kind owners.
pub mod cap {
    /// Built-in local route/loop decision controls.
    pub mod control {
        pub use crate::control::cap::resource_kinds::{
            LoopBreakKind, LoopContinueKind, RouteDecisionKind,
        };
    }

    pub use crate::control::cap::mint::{GenericCapToken, WireControlEffect, WireControlKind};
}

/// Wire payload codec surface.
pub mod wire {
    pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};
}

/// Transport I/O surface plus observation/detail owners.
pub mod transport {
    pub use crate::transport::{FrameLabel, Outgoing, PortOpen, Transport, TransportError};
}
