use core::marker::PhantomData;

#[derive(Clone, Copy)]
pub(crate) struct Owner<'rv, Step> {
    _brand: PhantomData<crate::control::brand::Guard<'rv>>,
    _step: PhantomData<Step>,
}

impl<'rv, Step> Owner<'rv, Step>
where
    Step: EpochType,
{
    #[inline]
    pub(crate) fn new(_brand: crate::control::brand::Guard<'rv>) -> Self {
        Self {
            _brand: PhantomData,
            _step: PhantomData,
        }
    }
}

// ============================================================================
// Operations that require a short-lived brand witness
// ============================================================================

#[derive(Clone, Copy, Default)]
pub(crate) struct EndpointEpoch<'r, Table: EpochTable> {
    _marker: PhantomData<&'r Table>,
}

impl<'r, Table: EpochTable> EndpointEpoch<'r, Table> {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Endpoint-Local Epoch Witness System
// ============================================================================

pub(crate) trait EpochType {}

/// Marker trait representing logical control-plane steps for a lane.
pub(crate) trait EpochStep: EpochType {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct E0;
impl EpochType for E0 {}
impl EpochStep for E0 {}

pub(crate) trait EpochTable {}

/// Compile-time epoch table carrying witnesses for each rendezvous lane.
pub(crate) struct EpochTbl<
    L0 = E0,
    L1 = E0,
    L2 = E0,
    L3 = E0,
    L4 = E0,
    L5 = E0,
    L6 = E0,
    L7 = E0,
    L8 = E0,
    L9 = E0,
    L10 = E0,
    L11 = E0,
    L12 = E0,
    L13 = E0,
    L14 = E0,
    L15 = E0,
> {
    _l0: PhantomData<L0>,
    _l1: PhantomData<L1>,
    _l2: PhantomData<L2>,
    _l3: PhantomData<L3>,
    _l4: PhantomData<L4>,
    _l5: PhantomData<L5>,
    _l6: PhantomData<L6>,
    _l7: PhantomData<L7>,
    _l8: PhantomData<L8>,
    _l9: PhantomData<L9>,
    _l10: PhantomData<L10>,
    _l11: PhantomData<L11>,
    _l12: PhantomData<L12>,
    _l13: PhantomData<L13>,
    _l14: PhantomData<L14>,
    _l15: PhantomData<L15>,
}

impl<L0, L1, L2, L3, L4, L5, L6, L7, L8, L9, L10, L11, L12, L13, L14, L15> EpochTable
    for EpochTbl<L0, L1, L2, L3, L4, L5, L6, L7, L8, L9, L10, L11, L12, L13, L14, L15>
where
    L0: EpochStep,
    L1: EpochStep,
    L2: EpochStep,
    L3: EpochStep,
    L4: EpochStep,
    L5: EpochStep,
    L6: EpochStep,
    L7: EpochStep,
    L8: EpochStep,
    L9: EpochStep,
    L10: EpochStep,
    L11: EpochStep,
    L12: EpochStep,
    L13: EpochStep,
    L14: EpochStep,
    L15: EpochStep,
{
}
