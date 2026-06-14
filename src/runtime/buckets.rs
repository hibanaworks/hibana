pub mod program {
    pub use crate::global::program::Projectable;
    pub use crate::global::role_program::{RoleProgram, project};
}

/// Protocol-neutral identifiers used by runtime crates.
pub mod ids {
    pub use crate::session::types::SessionId;
}

pub use crate::observe::core::TapEvent;
pub use crate::runtime_core::config::{Clock, Config, CounterClock};
pub use crate::runtime_core::consts::RING_EVENTS;

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

/// Resolver and decision-input surface for explicit route resolution.
pub mod resolver {
    pub use crate::session::cluster::core::{
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
        FrameHeader, FrameLabel, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError,
    };
}
