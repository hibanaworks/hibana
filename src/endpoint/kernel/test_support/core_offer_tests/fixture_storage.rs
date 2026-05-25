use crate::endpoint::kernel::core::offer_regression_tests::cases::*;

macro_rules! offer_fixture {
    ($size:expr, $clock:ident, $config:ident) => {
        let mut __offer_fixture = acquire_offer_fixture::<$size>();
        let $clock = __offer_fixture.clock();
        let $config = __offer_fixture.config();
    };
}

macro_rules! with_offer_cluster {
    ($clock:expr, $cluster_ty:ty, $cluster_ref:ident, $body:block) => {{ with_offer_cluster_slot::<$cluster_ty, _>($clock, |$cluster_ref| $body) }};
}

macro_rules! with_offer_value_slot {
    ($value_ty:ty, $slot:ident, $body:block) => {{
        with_offer_value_slot_storage(stringify!($slot), |storage, occupied| {
            with_offer_value_storage::<$value_ty, _>(storage, occupied, |$slot| $body)
        })
    }};
}

mod fixture_storage;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use fixture_storage::*;

mod transport_fixtures;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use transport_fixtures::*;

mod binding_fixtures;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use binding_fixtures::*;
