pub mod program {
    pub use crate::global::program::Projectable;
    pub use crate::global::role_program::{RoleProgram, project};
}

/// Protocol-neutral identifiers used by integration crates.
pub mod ids {
    pub use crate::control::types::SessionId;
    pub use crate::eff::EffIndex;
}

/// Everyday runtime setup owners for caller-provided storage and clocks.
pub mod runtime {
    pub use crate::observe::core::TapEvent;
    pub use crate::runtime::config::{Clock, Config, CounterClock, RuntimeStorage};
    pub use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse, RING_EVENTS};
}

/// Read-only tap observation surface.
pub mod tap {
    pub use crate::observe::core::{Evidence, TapEvent, TapPort};
    pub use crate::observe::ids::{
        TRANSPORT_FAULT, TRANSPORT_FAULT_CAPACITY, TRANSPORT_FAULT_DEADLINE,
        TRANSPORT_FAULT_FAILED, TRANSPORT_FAULT_OFFLINE, TRANSPORT_FRAME, TRANSPORT_MISMATCH,
        TRANSPORT_MISMATCH_LABEL, TRANSPORT_MISMATCH_LANE, TRANSPORT_MISMATCH_PEER_ROLE,
        TRANSPORT_MISMATCH_SESSION, TRANSPORT_MISMATCH_SOURCE_ROLE,
    };
}

/// Resolver and decision-input surface for dynamic policy.
pub mod policy {
    pub use crate::control::cluster::core::{
        DecisionArm, DecisionResolution, ResolverError, ResolverRef,
    };
}

/// Wire payload codec surface.
pub mod wire {
    pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};
}

/// Transport I/O surface plus observation/detail owners.
pub mod transport {
    pub use crate::transport::{
        FrameHeader, FrameLabel, IngressEvidence, Outgoing, PortOpen, ReceivedFrame, Transport,
        TransportError,
    };
}
