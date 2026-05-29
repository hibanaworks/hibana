/// Crate-private compile-time role domain size.
pub(crate) const ROLE_DOMAIN_SIZE: usize = 16;

/// Marker trait exposing the numeric role index.
pub trait RoleMarker {
    const INDEX: u8;
}

impl<const ROLE_INDEX: u8> RoleMarker for crate::g::Role<ROLE_INDEX> {
    const INDEX: u8 = ROLE_INDEX;
}

/// Trait implemented by every role type participating in a protocol.
pub trait KnownRole {
    const INDEX: u8;
}

impl<T: RoleMarker> KnownRole for T {
    const INDEX: u8 = T::INDEX;
}
