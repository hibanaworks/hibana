//! Runtime configuration describing the single slab envelope Hibana owns.
//!
//! Callers provide one mutable slab. Rendezvous initialization carves its
//! runtime envelope from that slab so Pico SRAM accounting has one authority
//! source.

use core::{fmt, ops::Range};

/// Borrowed resources required by the runtime.
pub struct Config<'a> {
    pub(crate) slab: &'a mut [u8],
}

impl<'a> fmt::Debug for Config<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("slab_bytes", &self.slab.len())
            .finish()
    }
}

impl<'a> Config<'a> {
    /// Borrow the runtime resources used by attach.
    ///
    /// Runtime sizing that follows from the projected program is derived by the
    /// attach path. Callers provide only the runtime slab; they do not choose
    /// lane windows, endpoint slot counts, diagnostics capacity, or hidden wait
    /// fuses.
    pub fn from_resources(slab: &'a mut [u8]) -> Self {
        Self { slab }
    }

    /// Zero-width lane domain materialized before a projected role descriptor exists.
    ///
    /// Lane legality and lane storage sizing are owned by projection metadata.
    /// Public config therefore starts with no materialized lane slots; endpoint
    /// attach expands the rendezvous to the role descriptor's lane span.
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
        let _: Config<'_> = Config::from_resources(&mut slab);
        let lane_range = Config::initial_lane_range();
        assert_eq!(lane_range, 0..0);
    }
}
