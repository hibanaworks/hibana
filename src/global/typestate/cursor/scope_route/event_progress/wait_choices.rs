use super::{EventAdmissionError, ScopeId};

type UnknownArm<'a> = dyn FnMut(ScopeId, bool) -> Option<u8> + 'a;

/// A caller may wait if any compatible completion of the choices inspected
/// by its predecessors leaves its held views free. Guesses remain local to
/// this read-only query. They never select an arm for protocol execution.
pub(super) fn wait_across_choices(
    scopes: usize,
    classify: &mut dyn FnMut(&mut UnknownArm<'_>) -> EventAdmissionError,
) -> EventAdmissionError {
    let mut first = None;
    let partial = classify(&mut |scope, fresh| {
        first.get_or_insert((scope, fresh));
        None
    });
    let Some(first) = first else { return partial };
    // A single unresolved decision needs no search storage, however large
    // the program that contains it. Stop as soon as one arm permits waiting.
    let mut other_decision = false;
    let mut blocked = EventAdmissionError::PredecessorPending;
    for arm in [0, 1] {
        let mut complete = true;
        let verdict = classify(&mut |scope, fresh| {
            if (scope, fresh) == first {
                Some(arm)
            } else {
                complete = false;
                None
            }
        });
        if complete && verdict == EventAdmissionError::OtherViewPending {
            return verdict;
        }
        other_decision |= !complete;
        if verdict == EventAdmissionError::HeldPredecessorPending {
            blocked = verdict;
        }
    }
    if !other_decision {
        return blocked;
    }
    // Each source route has at most a current and a fresh visit in this
    // query. Small descriptors fit both sets of decisions in one word.
    const DOMAIN_WORDS: usize = (2 * ScopeId::LOCAL_CAPACITY as usize).div_ceil(u64::BITS as usize);
    const MID_WORDS: usize = DOMAIN_WORDS.isqrt();
    let words = (scopes * 2).div_ceil(u64::BITS as usize);
    if words <= 1 {
        search::<1>(classify)
    } else if words <= MID_WORDS {
        search::<MID_WORDS>(classify)
    } else {
        search::<DOMAIN_WORDS>(classify)
    }
}

/// Descend the decision tree in first-read order. Replaying a leaf up to its
/// last left decision retains exactly the assignments of that prefix before
/// visiting the right subtree. Space depends on the packed scope domain,
/// independently of call depth, repeated reads or the number of paths.
#[inline(never)]
fn search<const WORDS: usize>(
    classify: &mut dyn FnMut(&mut UnknownArm<'_>) -> EventAdmissionError,
) -> EventAdmissionError {
    let mut right = [0u64; WORDS];
    let mut verdict = EventAdmissionError::PredecessorPending;
    loop {
        let mut seen = [0u64; WORDS];
        let (outcome, last_left) = walk(classify, &right, &mut seen, None);
        match outcome {
            EventAdmissionError::OtherViewPending => return EventAdmissionError::OtherViewPending,
            EventAdmissionError::HeldPredecessorPending => {
                verdict = EventAdmissionError::HeldPredecessorPending;
            }
            EventAdmissionError::PredecessorPending => {}
            EventAdmissionError::Invariant | EventAdmissionError::EntryViewUnavailable => {
                crate::invariant()
            }
        }
        let Some(pivot) = last_left else {
            return verdict;
        };
        seen.fill(0);
        walk(classify, &right, &mut seen, Some(pivot));
        for (bits, prefix) in right.iter_mut().zip(&seen) {
            *bits &= *prefix;
        }
        right[pivot / u64::BITS as usize] |= 1u64 << (pivot % u64::BITS as usize);
    }
}

fn walk(
    classify: &mut dyn FnMut(&mut UnknownArm<'_>) -> EventAdmissionError,
    right: &[u64],
    seen: &mut [u64],
    stop_before: Option<usize>,
) -> (EventAdmissionError, Option<usize>) {
    let mut prefix = true;
    let mut last_left = None;
    let outcome = classify(&mut |scope, fresh| {
        let decision = usize::from(scope.local_ordinal()) * 2 + usize::from(fresh);
        let word = decision / u64::BITS as usize;
        let bit = 1u64 << (decision % u64::BITS as usize);
        let arm = u8::from(right[word] & bit != 0);
        if Some(decision) == stop_before {
            prefix = false;
        }
        if prefix && seen[word] & bit == 0 {
            seen[word] |= bit;
            if arm == 0 {
                last_left = Some(decision);
            }
        }
        Some(arm)
    });
    (outcome, last_left)
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use EventAdmissionError::{HeldPredecessorPending, OtherViewPending, PredecessorPending};

    /// The first arm changes the order in which the two visits are read.
    /// Rereading its result must follow the same path on every replay.
    fn compare(scopes: usize, leaves: &[EventAdmissionError; 8]) {
        let outer = ScopeId::route(0);
        let inner = ScopeId::route((scopes - 1) as u16);
        let actual = wait_across_choices(scopes, &mut |read| {
            let Some(arm) = read(outer, false) else {
                return PredecessorPending;
            };
            let fresh_first = arm == 1;
            let Some(first) = read(inner, fresh_first) else {
                return PredecessorPending;
            };
            assert_eq!(read(outer, false), Some(arm));
            let Some(second) = read(inner, !fresh_first) else {
                return HeldPredecessorPending;
            };
            assert_eq!(read(inner, fresh_first), Some(first));
            let (current, fresh) = if fresh_first {
                (second, first)
            } else {
                (first, second)
            };
            leaves[usize::from(arm * 4 + current * 2 + fresh)]
        });
        let expected = if leaves.contains(&OtherViewPending) {
            OtherViewPending
        } else if leaves.contains(&HeldPredecessorPending) {
            HeldPredecessorPending
        } else {
            PredecessorPending
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn decision_replay_matches_every_three_choice_verdict_table() {
        for table in 0..3u16.pow(8) {
            let mut digits = table;
            let leaves = core::array::from_fn(|_| {
                let verdict = [PredecessorPending, HeldPredecessorPending, OtherViewPending]
                    [usize::from(digits % 3)];
                digits /= 3;
                verdict
            });
            compare(2, &leaves);
        }
        // Also exercise the last bit of the largest packed domain and the
        // intermediate storage size without multiplying the truth-table work.
        for scopes in [64, usize::from(ScopeId::LOCAL_CAPACITY)] {
            for reachable in 0..8 {
                let mut leaves = [PredecessorPending; 8];
                leaves[reachable] = OtherViewPending;
                compare(scopes, &leaves);
            }
        }
    }
}
