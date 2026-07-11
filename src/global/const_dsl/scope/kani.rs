use super::{ScopeId, ScopeKind};

#[kani::proof]
fn scope_id_decoding_accepts_exact_compact_domain() {
    let raw: u16 = kani::any();
    let expected = raw == u16::MAX
        || ((raw & 0x8000) == 0 && ((raw >> 13) & 0b11) <= ScopeKind::Parallel as u16);
    let decoded = ScopeId::decode_raw(raw);

    assert!(decoded.is_some() == expected);
    if let Some(scope) = decoded {
        assert!(scope.raw() == raw);
        assert!(scope.is_none() == (raw == u16::MAX));
        if !scope.is_none() {
            assert!(scope.local_ordinal() == raw & 0x1fff);
        }
    }
}
