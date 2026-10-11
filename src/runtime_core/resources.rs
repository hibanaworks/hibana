//! Runtime resource substrate owned by [`crate::runtime::SessionKit`].
//!
//! Public callers provide one mutable slab directly to `SessionKit::rendezvous`.
//! The runtime keeps this private wrapper only while carving resident storage.

use core::{fmt, ops::Range};

pub(crate) struct RuntimeResources<'a> {
    pub(crate) slab: &'a mut [u8],
}

impl<'a> fmt::Debug for RuntimeResources<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeResources")
            .field("slab_bytes", &self.slab.len())
            .finish()
    }
}

impl<'a> RuntimeResources<'a> {
    pub(crate) fn new(slab: &'a mut [u8]) -> Self {
        Self { slab }
    }

    pub(crate) fn initial_lane_range() -> Range<u16> {
        0..0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_defer_lane_domain_until_projected_descriptor() {
        let mut slab = [0u8; 256];
        let _: RuntimeResources<'_> = RuntimeResources::new(&mut slab);
        let lane_range = RuntimeResources::initial_lane_range();
        assert_eq!(lane_range, 0..0);
    }
}
