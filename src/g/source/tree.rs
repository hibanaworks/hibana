//! The typed choreography source consumed by lowering and ownership checking.
//!
//! Logical endpoint identity is separate from its current actor. This tree is
//! compile-time input; neither it nor its ownership queries belongs in a
//! resident endpoint or a transport frame.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointKey {
    pub component: u16,
    pub role: u8,
}

impl EndpointKey {
    pub const fn same(self, other: Self) -> bool {
        self.component == other.component && self.role == other.role
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    Wire { schema: u32 },
    Endpoint(EndpointKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub from: EndpointKey,
    pub to: EndpointKey,
    pub label: u8,
    pub payload: Payload,
}

#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    Send(Message),
    Seq(&'a Self, &'a Self),
    Par(&'a Self, &'a Self),
    Route {
        left: &'a Self,
        right: &'a Self,
        resolver: Option<u16>,
    },
    Roll(&'a Self),
}

impl Source<'_> {
    pub const fn has_delegation(&self) -> bool {
        match self {
            Self::Send(message) => matches!(message.payload, Payload::Endpoint(_)),
            Self::Seq(left, right) | Self::Par(left, right) | Self::Route { left, right, .. } => {
                left.has_delegation() || right.has_delegation()
            }
            Self::Roll(body) => body.has_delegation(),
        }
    }

    pub const fn event_count(&self) -> usize {
        match self {
            Self::Send(_) => 1,
            Self::Seq(left, right) | Self::Par(left, right) | Self::Route { left, right, .. } => {
                left.event_count() + right.event_count()
            }
            Self::Roll(body) => body.event_count(),
        }
    }

    pub const fn uses(&self, key: EndpointKey) -> bool {
        match self {
            Self::Send(message) => {
                message.from.same(key)
                    || message.to.same(key)
                    || matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
            }
            Self::Seq(left, right) | Self::Par(left, right) | Self::Route { left, right, .. } => {
                left.uses(key) || right.uses(key)
            }
            Self::Roll(body) => body.uses(key),
        }
    }

    pub const fn transfers(&self, key: EndpointKey) -> bool {
        match self {
            Self::Send(message) => {
                matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
            }
            Self::Seq(left, right) | Self::Par(left, right) | Self::Route { left, right, .. } => {
                left.transfers(key) || right.transfers(key)
            }
            Self::Roll(body) => body.transfers(key),
        }
    }
}
