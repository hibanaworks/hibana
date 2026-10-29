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
pub struct Summary {
    events: usize,
    transferred_roles: [u64; (u8::MAX as usize + 1).div_ceil(u64::BITS as usize)],
}

impl Summary {
    const fn pair(left: &Source<'_>, right: &Source<'_>) -> Self {
        Self {
            events: match left.event_count().checked_add(right.event_count()) {
                Some(events) => events,
                None => panic!("source event count overflow"),
            },
            transferred_roles: [
                left.transfer_role_word(0) | right.transfer_role_word(0),
                left.transfer_role_word(1) | right.transfer_role_word(1),
                left.transfer_role_word(2) | right.transfer_role_word(2),
                left.transfer_role_word(3) | right.transfer_role_word(3),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    Send(Message),
    Seq(&'a Self, &'a Self, Summary),
    Par(&'a Self, &'a Self, Summary),
    Route {
        left: &'a Self,
        right: &'a Self,
        resolver: Option<u16>,
        summary: Summary,
    },
    Roll(&'a Self, Summary),
}

impl<'a> Source<'a> {
    pub const fn seq(left: &'a Self, right: &'a Self) -> Self {
        Self::Seq(left, right, Summary::pair(left, right))
    }

    pub const fn par(left: &'a Self, right: &'a Self) -> Self {
        Self::Par(left, right, Summary::pair(left, right))
    }

    pub const fn route(left: &'a Self, right: &'a Self, resolver: Option<u16>) -> Self {
        Self::Route {
            left,
            right,
            resolver,
            summary: Summary::pair(left, right),
        }
    }

    pub const fn roll(body: &'a Self) -> Self {
        Self::Roll(
            body,
            Summary {
                events: body.event_count(),
                transferred_roles: [
                    body.transfer_role_word(0),
                    body.transfer_role_word(1),
                    body.transfer_role_word(2),
                    body.transfer_role_word(3),
                ],
            },
        )
    }
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
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
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

    /// Next contiguous source range ordered before `event`, starting at `lower`.
    /// Parallel siblings and alternative arms contribute no predecessor range.
    pub const fn predecessor_range(&self, event: usize, lower: usize) -> Option<(usize, usize)> {
        if lower >= event {
            return None;
        }
        match self {
            Self::Send(_) => None,
            Self::Roll(body, _) => body.predecessor_range(event, lower),
            Self::Seq(left, right, _)
            | Self::Par(left, right, _)
            | Self::Route { left, right, .. } => {
                let split = left.event_count();
                if event < split {
                    left.predecessor_range(event, lower)
                } else if matches!(self, Self::Seq(..)) && lower < split {
                    Some((lower, split))
                } else {
                    let lower = lower.saturating_sub(split);
                    match right.predecessor_range(event - split, lower) {
                        Some((start, end)) => Some((start + split, end + split)),
                        None => None,
                    }
                }
            }
        }
    }

    pub const fn has_delegation(&self) -> bool {
        self.transfer_role_word(0)
            | self.transfer_role_word(1)
            | self.transfer_role_word(2)
            | self.transfer_role_word(3)
            != 0
    }

    // Source-only subtree index over the complete u8 role domain. A matching
    // bit still requires the leaf's full component and role to match.
    const fn transfer_role_word(&self, word: usize) -> u64 {
        match self {
            Self::Send(Message {
                payload: Payload::Endpoint(key),
                ..
            }) if key.role as usize / u64::BITS as usize == word => {
                1 << (key.role as usize % u64::BITS as usize)
            }
            Self::Send(_) => 0,
            Self::Seq(_, _, summary)
            | Self::Par(_, _, summary)
            | Self::Route { summary, .. }
            | Self::Roll(_, summary) => summary.transferred_roles[word],
        }
    }

    /// The typed constructors compute this once for each subtree.
    pub const fn event_count(&self) -> usize {
        match self {
            Self::Send(_) => 1,
            Self::Seq(_, _, summary)
            | Self::Par(_, _, summary)
            | Self::Route { summary, .. }
            | Self::Roll(_, summary) => summary.events,
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

    pub(super) const fn may_transfer(&self, key: EndpointKey) -> bool {
        self.transfer_role_word(key.role as usize / u64::BITS as usize)
            & (1 << (key.role as usize % u64::BITS as usize))
            != 0
    }

    pub const fn transfers(&self, key: EndpointKey) -> bool {
        if !self.may_transfer(key) {
            return false;
        }
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
        if !self.may_transfer(key) {
            return None;
        }
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
