//! Build protocol subtrees performed through a received endpoint.
use super::{Protocol, message};

#[derive(Debug)]
pub(super) enum Body {
    Exchange,
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>),
    Route(Box<Self>, Box<Self>),
    Resolved(Box<Self>, Box<Self>),
    Roll(Box<Self>),
    Receive(Box<Self>),
    Forward(Box<Self>),
}

pub(super) struct Counts {
    local: Vec<usize>,
    mobile: Vec<usize>,
}
impl Counts {
    pub(super) fn new(budget: usize) -> Self {
        let mut counts = Self {
            local: vec![1],
            mobile: vec![1],
        };
        for cost in 1..=budget {
            let mut local = 2 * counts.local[cost - 1];
            let mut mobile = 3 * counts.mobile[cost - 1];
            for left in 0..cost {
                let right = cost - 1 - left;
                local += 4 * counts.local[left] * counts.local[right];
                mobile += 3 * counts.mobile[left] * counts.mobile[right]
                    + counts.local[left] * counts.local[right];
            }
            counts.local.push(local);
            counts.mobile.push(mobile);
        }
        counts
    }
    pub(super) fn total(&self) -> usize {
        self.mobile.iter().sum()
    }
    pub(super) fn shape(&self, mut rank: usize) -> Body {
        let mut cost = 0;
        while rank >= self.mobile[cost] {
            rank -= self.mobile[cost];
            cost += 1;
        }
        self.at(cost, rank, true)
    }
    fn at(&self, cost: usize, mut rank: usize, mobile: bool) -> Body {
        if cost == 0 {
            assert_eq!(rank, 0);
            return Body::Exchange;
        }
        let rows = if mobile { &self.mobile } else { &self.local };
        for operator in 0..if mobile { 3 } else { 2 } {
            if rank < rows[cost - 1] {
                let child = Box::new(self.at(cost - 1, rank, mobile));
                return match operator {
                    0 => Body::Roll(child),
                    1 => Body::Receive(child),
                    2 => Body::Forward(child),
                    _ => unreachable!(),
                };
            }
            rank -= rows[cost - 1];
        }
        for left in 0..cost {
            let right = cost - 1 - left;
            for operator in 0..4 {
                // A parallel borrow cannot move its parent endpoint. Each side
                // may contain arbitrary local operations, choices and rolls.
                let child_mobile = mobile && operator != 1;
                let children = if child_mobile {
                    &self.mobile
                } else {
                    &self.local
                };
                let width = children[right];
                let pairs = children[left] * width;
                if rank < pairs {
                    let a = Box::new(self.at(left, rank / width, child_mobile));
                    let b = Box::new(self.at(right, rank % width, child_mobile));
                    return match operator {
                        0 => Body::Seq(a, b),
                        1 => Body::Par(a, b),
                        2 => Body::Route(a, b),
                        3 => Body::Resolved(a, b),
                        _ => unreachable!(),
                    };
                }
                rank -= pairs;
            }
        }
        unreachable!("delegated body rank")
    }
}

impl Body {
    // Every global below finally returns its single resource to role 0.
    // A roll at another holder needs fresh causal permission for service 2.
    // A prefix outside that roll grants no permission for its later visits.
    pub(super) fn has_uninformed_reentry(&self, holder: u8, final_owner: u8) -> bool {
        match self {
            Self::Exchange => false,
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) | Self::Resolved(a, b) => {
                a.has_uninformed_reentry(holder, final_owner)
                    || b.has_uninformed_reentry(holder, final_owner)
            }
            Self::Roll(body) => {
                (holder != final_owner && body.has_uninformed_origin())
                    || body.has_uninformed_reentry(holder, final_owner)
            }
            Self::Receive(body) => body.has_uninformed_reentry(holder, final_owner),
            Self::Forward(body) => body.has_uninformed_reentry(holder ^ 1, final_owner),
        }
    }

    fn has_uninformed_origin(&self) -> bool {
        match self {
            Self::Receive(_) => true,
            Self::Exchange | Self::Route(..) | Self::Resolved(..) => false,
            Self::Seq(a, _) => a.has_uninformed_origin(),
            Self::Par(a, b) => a.has_uninformed_origin() || b.has_uninformed_origin(),
            Self::Roll(body) | Self::Forward(body) => body.has_uninformed_origin(),
        }
    }

    pub(super) fn build(&self, next: &mut u8, resolver: &mut u16, holder: u8) -> Protocol {
        match self {
            Self::Exchange => Protocol::Seq(
                Box::new(message(next, 3, 2, false)),
                Box::new(message(next, 2, 3, false)),
            ),
            Self::Seq(a, b) | Self::Par(a, b) => {
                let a = Box::new(a.build(next, resolver, holder));
                let b = Box::new(b.build(next, resolver, holder));
                if matches!(self, Self::Seq(..)) {
                    Protocol::Seq(a, b)
                } else {
                    Protocol::Par(a, b)
                }
            }
            Self::Route(a, b) | Self::Resolved(a, b) => {
                let id = *resolver;
                if matches!(self, Self::Resolved(..)) {
                    *resolver += 1;
                }
                let arm = |body: &Self, next: &mut u8, resolver: &mut u16| {
                    // The service and the other possible holder learn this
                    // choice before any optional onward delegation.
                    Protocol::Seq(
                        Box::new(message(next, 3, 2, false)),
                        Box::new(Protocol::Seq(
                            Box::new(message(next, 3, holder ^ 1, false)),
                            Box::new(body.build(next, resolver, holder)),
                        )),
                    )
                };
                let a = Box::new(arm(a, next, resolver));
                let b = Box::new(arm(b, next, resolver));
                if matches!(self, Self::Resolved(..)) {
                    Protocol::Resolved(a, b, id)
                } else {
                    Protocol::Route(a, b)
                }
            }
            Self::Roll(body) => Protocol::Roll(Box::new(body.build(next, resolver, holder))),
            Self::Receive(body) => Protocol::Seq(
                Box::new(message(next, 2, 3, false)),
                Box::new(Protocol::Seq(
                    Box::new(message(next, 3, 2, false)),
                    Box::new(body.build(next, resolver, holder)),
                )),
            ),
            Self::Forward(body) => Protocol::Seq(
                Box::new(message(next, holder, holder ^ 1, true)),
                Box::new(Protocol::Seq(
                    Box::new(body.build(next, resolver, holder ^ 1)),
                    Box::new(message(next, holder ^ 1, holder, true)),
                )),
            ),
        }
    }
    pub(super) fn global(&self, next: &mut u8, resolver: &mut u16) -> Protocol {
        Protocol::Seq(
            Box::new(message(next, 0, 1, true)),
            Box::new(Protocol::Seq(
                Box::new(self.build(next, resolver, 1)),
                Box::new(message(next, 1, 0, true)),
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Body;

    fn remote_loop() -> Body {
        Body::Roll(Box::new(Body::Receive(Box::new(Body::Exchange))))
    }

    #[test]
    fn an_outer_choice_does_not_authorize_later_loop_visits() {
        let outer = Body::Resolved(Box::new(remote_loop()), Box::new(Body::Exchange));
        let per_visit = Body::Roll(Box::new(Body::Resolved(
            Box::new(Body::Receive(Box::new(Body::Exchange))),
            Box::new(Body::Exchange),
        )));
        assert!(outer.has_uninformed_reentry(1, 0));
        assert!(!per_visit.has_uninformed_reentry(1, 0));
    }

    #[test]
    fn a_parallel_exchange_is_not_a_causal_permit_for_the_other_arm() {
        let parallel = Body::Roll(Box::new(Body::Par(
            Box::new(Body::Exchange),
            Box::new(Body::Receive(Box::new(Body::Exchange))),
        )));
        let sequential = Body::Roll(Box::new(Body::Seq(
            Box::new(Body::Exchange),
            Box::new(Body::Receive(Box::new(Body::Exchange))),
        )));
        assert!(parallel.has_uninformed_reentry(1, 0));
        assert!(!sequential.has_uninformed_reentry(1, 0));
    }

    #[test]
    fn reentry_at_the_final_owner_survives_a_temporary_absence() {
        let returned = Body::Forward(Box::new(remote_loop()));
        assert!(!returned.has_uninformed_reentry(1, 0));
        let outer = Body::Roll(Box::new(returned));
        assert!(outer.has_uninformed_reentry(1, 0));
    }

    #[test]
    fn swapping_holders_preserves_the_classification_of_every_bounded_body() {
        let counts = super::Counts::new(3);
        for rank in 0..counts.total() {
            let body = counts.shape(rank);
            assert_eq!(
                body.has_uninformed_reentry(1, 0),
                body.has_uninformed_reentry(0, 1),
                "{body:?}"
            );
        }
    }
}
