use super::{AssocTable, ENTRY_COUNT_MAX, next_attachment_count};

#[kani::proof]
fn packed_state_preserves_full_count_and_fault_code() {
    let candidate = (kani::any::<u16>(), kani::any::<u8>());
    let (count, fault) = if candidate.0 <= ENTRY_COUNT_MAX && candidate.1 <= 5 {
        candidate
    } else {
        (0, 0)
    };

    let state = AssocTable::entry_state(count, fault);
    assert!(AssocTable::entry_count(state) == count);
    assert!(AssocTable::entry_fault_code(state) == fault);
}

#[kani::proof]
fn attachment_count_accepts_exact_full_role_domain() {
    let candidate: u16 = kani::any();
    let current = if candidate != 0 && candidate <= ENTRY_COUNT_MAX {
        candidate
    } else {
        1
    };

    let next = next_attachment_count(current);
    assert!(next.is_some() == (current < ENTRY_COUNT_MAX));
    if let Some(next) = next {
        assert!(next == current + 1);
        assert!(next <= ENTRY_COUNT_MAX);
    }
    kani::cover!(current == u8::MAX as u16);
    kani::cover!(current == ENTRY_COUNT_MAX);
}

#[kani::proof]
fn attachment_increment_preserves_packed_fault_code() {
    let candidate = (kani::any::<u16>(), kani::any::<u8>());
    let (current, fault) = if candidate.0 != 0 && candidate.0 < ENTRY_COUNT_MAX && candidate.1 <= 5
    {
        candidate
    } else {
        (1, 0)
    };

    let raw = AssocTable::entry_state(current, fault);
    let next = next_attachment_count(AssocTable::entry_count(raw)).expect("bounded increment");
    let updated = AssocTable::entry_state(next, AssocTable::entry_fault_code(raw));
    assert!(AssocTable::entry_count(updated) == current + 1);
    assert!(AssocTable::entry_fault_code(updated) == fault);
}

#[kani::proof]
fn attachment_count_allows_256_and_rejects_257() {
    let count_256 = next_attachment_count(u8::MAX as u16).expect("256th attachment");
    assert!(count_256 == ENTRY_COUNT_MAX);
    assert!(next_attachment_count(count_256).is_none());
}
