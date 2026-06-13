use super::common::*;

#[test]
fn array_map_unsafe_boundaries_are_explicit_and_panic_safe() {
    let map = read("src/session/lease/map.rs");
    let lease_core = read("src/session/lease/core.rs");

    assert!(
        map.contains("pub(crate) unsafe fn try_push_with")
            && map.contains(
                "`init` must fully initialize the provided slot before returning `Ok(())`"
            ),
        "ArrayMap::try_push_with must expose its MaybeUninit invariant as an unsafe contract"
    );
    assert!(
        lease_core.contains(
            "SAFETY: The key written before entry initialization is `RendezvousId: Copy`"
        ) && lease_core.contains(".try_push_with("),
        "ArrayMap::try_push_with callers must document the exact initialized-state invariant"
    );
    assert!(
        !map.contains(
            "assume_init_drop();\n                    self.entries[i].write((key, value));"
        ),
        "ArrayMap::insert must not drop a live slot before replacement is committed"
    );
    assert!(
        !map.contains("pub(crate) fn retain(") && !map.contains("fn retain("),
        "ArrayMap must not retain a generic panic-unsafe compactor"
    );
    assert!(
        !map.contains("let forbidden_len = self.len;\n        // compact retained entries later"),
        "ArrayMap::retain must not contain a deferred-compaction shape that leaves len inconsistent during unwinding"
    );
}
