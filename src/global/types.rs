use core::marker::PhantomData;

/// Crate-private compile-time role domain size.
pub(crate) const ROLE_DOMAIN_SIZE: usize = 16;

/// Compile-time role marker (0 ≤ IDX < 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Role<const ROLE_INDEX: u8>;

/// Marker trait exposing the numeric role index.
pub trait RoleMarker {
    const INDEX: u8;
}

impl<const ROLE_INDEX: u8> RoleMarker for Role<ROLE_INDEX> {
    const INDEX: u8 = ROLE_INDEX;
}

/// Trait implemented by every role type participating in a protocol.
pub trait KnownRole {
    const INDEX: u8;
}

impl<T: RoleMarker> KnownRole for T {
    const INDEX: u8 = T::INDEX;
}

/// Marker trait for compile-time labels.
pub trait LabelTag {
    const VALUE: u8;
}

/// Concrete label marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LabelMarker<const LABEL_VALUE: u8>;

impl<const LABEL_VALUE: u8> LabelTag for LabelMarker<LABEL_VALUE> {
    const VALUE: u8 = LABEL_VALUE;
}

/// Phantom message descriptor tying a label to a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Message<Label, Payload, Control = ()>(PhantomData<(Label, Payload, Control)>);

/// Canonical message descriptor when the label is known as a const generic.
pub type Msg<const LOGICAL_LABEL: u8, Payload, Control = ()> =
    Message<LabelMarker<LOGICAL_LABEL>, Payload, Control>;
