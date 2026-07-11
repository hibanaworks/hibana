use super::ProgramAtomRow;

#[kani::proof]
fn program_atom_row_decoding_accepts_exact_domain() {
    let eff_idx: u16 = kani::any();
    let from: u8 = kani::any();
    let to: u8 = kani::any();
    let label: u8 = kani::any();
    let origin: u8 = kani::any();
    let lane: u8 = kani::any();
    let role_count: u8 = kani::any();

    let expected = (eff_idx as usize) < crate::eff::meta::MAX_EFF_NODES
        && role_count != 0
        && role_count <= crate::g::ROLE_DOMAIN_SIZE
        && from < role_count
        && to < role_count
        && origin <= 1;
    let decoded = ProgramAtomRow::decode(eff_idx, from, to, label, origin, lane, role_count);

    assert!(decoded.is_some() == expected);
    if let Some(row) = decoded {
        assert!(row.eff_idx == eff_idx);
        assert!(row.atom.from == from);
        assert!(row.atom.to == to);
        assert!(row.atom.label == label);
        assert!(row.atom.origin.packed_bits() == origin);
        assert!(row.atom.lane == lane);
    }
}
