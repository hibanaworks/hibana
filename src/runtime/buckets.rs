/// Project a global protocol and inspect its compiled local protocol data.
pub mod program {
    pub use crate::global::program::Projectable;
    pub use crate::global::role_program::{RoleProgram, project};
}

/// Session identifiers.
pub mod ids {
    pub use crate::session::types::SessionId;
}

/// Observe protocol and transport events without changing their execution.
pub mod tap {
    pub use crate::observe::core::{Evidence, TapEvent, TapPort};
    pub use crate::observe::ids::{
        ENDPOINT_RECV, ENDPOINT_SEND, ENDPOINT_SESSION, LANE_ACQUIRE, LANE_RELEASE, RESOLVER_AUDIT,
        ROUTE_ARM_SELECTION, TRANSPORT_FAULT, TRANSPORT_FAULT_CAPACITY, TRANSPORT_FAULT_DEADLINE,
        TRANSPORT_FAULT_FAILED, TRANSPORT_FAULT_OFFLINE, TRANSPORT_FRAME, TRANSPORT_MISMATCH,
        TRANSPORT_MISMATCH_LABEL, TRANSPORT_MISMATCH_LANE, TRANSPORT_MISMATCH_PEER_ROLE,
        TRANSPORT_MISMATCH_SESSION, TRANSPORT_MISMATCH_SOURCE_ROLE,
    };
}

/// Register application decisions for protocol choices.
pub mod resolver {
    pub use crate::session::cluster::core::{DecisionArm, ResolverError, ResolverRef};
}

/// Encode and validate message payloads in supplied buffers.
pub mod wire {
    pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};
}

/// Implement asynchronous I/O, frame buffers and resource bindings.
pub mod transport {
    pub use crate::transport::{
        EndpointBinding, FrameHeader, FrameLabel, Outgoing, PortOpen, ReceivedFrame, Transport,
        TransportError,
    };
}
