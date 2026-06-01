use super::common::*;

#[test]
fn array_map_unsafe_boundaries_are_explicit_and_panic_safe() {
    let map = read("src/control/lease/map.rs");
    let lease_core = read("src/control/lease/core.rs");

    assert!(
        map.contains("pub(crate) unsafe fn try_push_with")
            && map.contains(
                "`init` must fully initialize the provided slot before returning `Ok(())`"
            ),
        "ArrayMap::try_push_with must expose its MaybeUninit invariant as an unsafe contract"
    );
    assert!(
        lease_core.contains("SAFETY: The key written before delegation is `RendezvousId: Copy`")
            && lease_core.contains(".try_push_with("),
        "ArrayMap::try_push_with callers must document the exact initialized-state invariant"
    );
    assert!(
        !map.contains(
            "assume_init_drop();\n                    self.entries[i].write((key, value));"
        ),
        "ArrayMap::insert must not drop a live slot before replacement is committed"
    );
    assert!(
        map.contains("pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool)")
            && map.contains("V: Copy"),
        "ArrayMap::retain must stay constrained to Copy values instead of exposing a generic panic-unsafe compactor"
    );
    assert!(
        !map.contains("let old_len = self.len;\n        // compact retained entries later"),
        "ArrayMap::retain must not reintroduce a deferred-compaction shape that leaves len stale during unwinding"
    );
}
