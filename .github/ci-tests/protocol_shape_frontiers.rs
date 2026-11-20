//! Parallel repetitions followed by a choice on another logical direction.
use super::Protocol;

const PARALLEL_CONTINUATIONS: usize = 32;
pub(super) const COUNT: usize = PARALLEL_CONTINUATIONS + 1;

fn message(from: u8, to: u8, owned: bool) -> Protocol {
    Protocol::Message {
        from,
        to,
        event: 0,
        owned,
        contract: 0,
    }
}

fn seq(left: Protocol, right: Protocol) -> Protocol {
    Protocol::Seq(Box::new(left), Box::new(right))
}

fn choice(from: u8, to: u8, resolved: bool) -> Protocol {
    let left = Box::new(message(from, to, false));
    let right = Box::new(message(from, to, false));
    if resolved {
        Protocol::Resolved(left, right, 0)
    } else {
        Protocol::Route(left, right)
    }
}

fn source(rank: usize, resource_initiates: bool) -> Protocol {
    assert!(rank < PARALLEL_CONTINUATIONS);
    let owned = rank & 16 != 0;
    let outgoing = rank & 8 != 0;
    let receiver = if owned { 3 } else { 1 };
    let next_sender = if owned { 1 } else { 3 };
    let left_sender = if owned && resource_initiates {
        receiver
    } else {
        0
    };
    let left_receiver = if owned && resource_initiates {
        0
    } else {
        receiver
    };
    let right_sender = if owned && resource_initiates {
        receiver
    } else {
        2
    };
    let right_receiver = if owned && resource_initiates {
        2
    } else {
        receiver
    };
    let left = seq(
        Protocol::Roll(Box::new(choice(left_sender, left_receiver, rank & 1 != 0))),
        message(left_receiver, left_sender, false),
    );
    let right = seq(
        Protocol::Roll(Box::new(choice(
            right_sender,
            right_receiver,
            rank & 2 != 0,
        ))),
        message(right_receiver, right_sender, false),
    );
    let (from, to) = if outgoing {
        (receiver, next_sender)
    } else {
        (next_sender, receiver)
    };
    let body = seq(
        Protocol::Par(Box::new(left), Box::new(right)),
        choice(from, to, rank & 4 != 0),
    );
    if owned {
        seq(message(0, 1, true), seq(body, message(1, 0, true)))
    } else {
        body
    }
}

pub(super) fn protocol(rank: usize) -> (Protocol, String) {
    assert!(rank < COUNT);
    if rank == PARALLEL_CONTINUATIONS {
        // The file never owns resource 3. Its repository receipt proves that
        // the old holder cannot start another visit after the actual return.
        let retry = seq(
            message(3, 0, false),
            seq(
                message(0, 2, false),
                seq(
                    message(0, 3, false),
                    seq(message(2, 3, false), message(3, 0, false)),
                ),
            ),
        );
        return (
            seq(
                message(0, 1, true),
                seq(
                    Protocol::Roll(Box::new(retry)),
                    seq(message(1, 0, true), message(0, 2, false)),
                ),
            ),
            "remote-file-receive-after-permanent-resource-return".into(),
        );
    }
    (
        source(rank, true),
        format!(
            "parallel-roll-continuation/owned={}/outgoing={}/resolvers={}",
            rank & 16 != 0,
            rank & 8 != 0,
            rank & 7
        ),
    )
}

// An outside sender can restart these loops after the resource has returned
// to its original owner. Keep this source as a compile-fail counterexample.
pub(super) fn unpermitted_reentry(rank: usize) -> Option<Protocol> {
    (rank & 16 != 0).then(|| {
        let mut global = source(rank, false);
        super::syntax::number(&mut global);
        global
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_choice_and_direction_has_an_ordinary_and_a_delegated_case() {
        let mut sources = std::collections::BTreeSet::new();
        for rank in 0..COUNT {
            let (mut source, _) = protocol(rank);
            let (_, resolvers) = super::super::syntax::number(&mut source);
            if rank < PARALLEL_CONTINUATIONS {
                assert_eq!(resolvers, (rank & 7).count_ones() as u16);
                assert_eq!(super::super::context::has_transfer(&source), rank & 16 != 0);
            } else {
                assert_eq!(resolvers, 0);
                assert!(super::super::context::has_transfer(&source));
            }
            assert!(
                super::super::ownership::violations(&source).is_empty(),
                "rank={rank}"
            );
            assert!(sources.insert(source));
        }
        assert_eq!(sources.len(), COUNT);
    }
}
