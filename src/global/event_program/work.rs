//! Counts selected runtime queries in caller-owned event-program storage.
//! These counts describe Rust operations, independently of clocks and CPU
//! instructions. The module is included only with `--cfg hibana_profile`.
use core::cell::Cell;

macro_rules! metrics {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug)]
        #[repr(usize)]
        pub(crate) enum Metric { $($variant),+ }
        pub(crate) const NAMES: &[&str] = &[$($name),+];
    };
}

metrics! {
    OperationIdentity => "operation_identity_queries",
    Conflict => "conflict_queries",
    OutboundCandidate => "outbound_candidate_rows",
    InboundCandidate => "inbound_candidate_rows",
    FrameCandidate => "frame_candidate_queries",
    Admission => "admission_checks",
    Predecessor => "predecessor_rows",
    RouteRows => "route_row_queries",
}

#[derive(Debug)]
pub(crate) struct Counters {
    values: [Cell<u64>; NAMES.len()],
}

impl Counters {
    pub(crate) const fn new() -> Self {
        Self {
            values: [const { Cell::new(0) }; NAMES.len()],
        }
    }

    #[inline]
    pub(crate) fn hit(&self, metric: Metric) {
        let value = &self.values[metric as usize];
        value.replace(
            value
                .get()
                .checked_add(1)
                .expect("runtime work counter overflow"),
        );
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) fn report(&self) -> [(&'static str, u64); NAMES.len()] {
        core::array::from_fn(|index| (NAMES[index], self.values[index].get()))
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;

    #[test]
    fn only_the_observed_query_changes_its_counter() {
        let counters = Counters::new();
        counters.hit(Metric::OutboundCandidate);
        counters.hit(Metric::OutboundCandidate);
        counters.hit(Metric::RouteRows);
        for (name, count) in counters.report() {
            assert_eq!(
                count,
                match name {
                    "outbound_candidate_rows" => 2,
                    "route_row_queries" => 1,
                    _ => 0,
                }
            );
        }
        assert_eq!(
            Counters::new().report().map(|(_, count)| count),
            [0; NAMES.len()]
        );
    }

    #[test]
    #[should_panic(expected = "runtime work counter overflow")]
    fn overflow_cannot_be_reported_as_less_work() {
        let counters = Counters::new();
        counters.values[Metric::OperationIdentity as usize].replace(u64::MAX);
        counters.hit(Metric::OperationIdentity);
    }
}
