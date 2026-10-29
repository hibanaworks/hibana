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
    Seq(&'a Self, &'a Self, usize),
    Par(&'a Self, &'a Self, usize),
    Route {
        left: &'a Self,
        right: &'a Self,
        resolver: Option<u16>,
        events: usize,
    },
    Roll(&'a Self, usize),
}

impl Source<'_> {
    /// Source occurrence lookup; its ordinal does not impose execution order.
    pub const fn message_at(&self, event: usize) -> Message {
        match self {
            Self::Send(message) if event == 0 => *message,
            Self::Send(_) => panic!("source event outside fragment"),
            Self::Seq(left, right, _)
            | Self::Par(left, right, _)
            | Self::Route { left, right, .. } => {
                let split = left.event_count();
                if event < split {
                    left.message_at(event)
                } else {
                    right.message_at(event - split)
                }
            }
            Self::Roll(body, _) => body.message_at(event),
        }
    }

    /// Strict order comes from seq, never from enumeration, par, or two
    /// different route arms. Roll contributes its single-visit body order.
    pub const fn before(&self, first: usize, second: usize) -> bool {
        if first >= second {
            return false;
        }
        match self {
            Self::Send(_) => false,
            Self::Seq(left, right, _)
            | Self::Par(left, right, _)
            | Self::Route { left, right, .. } => {
                let split = left.event_count();
                if first < split && second < split {
                    left.before(first, second)
                } else if first >= split && second >= split {
                    right.before(first - split, second - split)
                } else {
                    matches!(self, Self::Seq(..)) && first < split && second >= split
                }
            }
            Self::Roll(body, _) => body.before(first, second),
        }
    }

    pub const fn has_delegation(&self) -> bool {
        match self {
            Self::Send(message) => matches!(message.payload, Payload::Endpoint(_)),
            Self::Seq(left, right, _)
            | Self::Par(left, right, _)
            | Self::Route { left, right, .. } => left.has_delegation() || right.has_delegation(),
            Self::Roll(body, _) => body.has_delegation(),
        }
    }

    /// The typed constructors compute this once for each subtree.
    pub const fn event_count(&self) -> usize {
        match self {
            Self::Send(_) => 1,
            Self::Seq(_, _, events)
            | Self::Par(_, _, events)
            | Self::Route { events, .. }
            | Self::Roll(_, events) => *events,
        }
    }

    pub const fn uses(&self, key: EndpointKey) -> bool {
        match self {
            Self::Send(message) => {
                message.from.same(key)
                    || message.to.same(key)
                    || matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
            }
            Self::Seq(left, right, _)
            | Self::Par(left, right, _)
            | Self::Route { left, right, .. } => left.uses(key) || right.uses(key),
            Self::Roll(body, _) => body.uses(key),
        }
    }

    pub const fn transfers(&self, key: EndpointKey) -> bool {
        match self {
            Self::Send(message) => {
                matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
            }
            Self::Seq(left, right, _)
            | Self::Par(left, right, _)
            | Self::Route { left, right, .. } => left.transfers(key) || right.transfers(key),
            Self::Roll(body, _) => body.transfers(key),
        }
    }

    /// The first source occurrence carrying this complete endpoint key.
    /// Choice arms are source alternatives; this query does not select one.
    pub const fn first_transfer(&self, key: EndpointKey) -> Option<usize> {
        match self {
            Self::Send(message) => {
                if matches!(message.payload, Payload::Endpoint(payload) if payload.same(key)) {
                    Some(0)
                } else {
                    None
                }
            }
            Self::Seq(left, right, _)
            | Self::Par(left, right, _)
            | Self::Route { left, right, .. } => match left.first_transfer(key) {
                Some(event) => Some(event),
                None => match right.first_transfer(key) {
                    Some(event) => Some(left.event_count() + event),
                    None => None,
                },
            },
            Self::Roll(body, _) => body.first_transfer(key),
        }
    }
}
