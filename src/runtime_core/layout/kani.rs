use super::{checked_align_offset, checked_align_up, u32_word_count};

#[kani::proof]
fn u32_word_count_is_exact_over_the_complete_usize_domain() {
    let bits: usize = kani::any();
    let words = u32_word_count(bits);

    if bits == 0 {
        assert!(words == 0);
    } else {
        assert!(words == ((bits - 1) / u32::BITS as usize) + 1);
    }
}

#[kani::proof]
fn checked_alignment_is_exact_over_the_complete_usize_domain() {
    let value: usize = kani::any();
    let shift: u8 = kani::any();
    kani::assume(u32::from(shift) < usize::BITS);
    let align = 1usize << shift;
    let mask = align - 1;

    let aligned = checked_align_up(value, align);
    assert!(aligned.is_some() == value.checked_add(mask).is_some());
    if let Some(aligned) = aligned {
        assert!(aligned >= value);
        assert!(aligned - value < align);
        assert!(aligned & mask == 0);
    }
}

#[kani::proof]
fn checked_absolute_offset_alignment_is_exact_and_never_wraps() {
    let base: usize = kani::any();
    let offset: usize = kani::any();
    let shift: u8 = kani::any();
    kani::assume(u32::from(shift) < usize::BITS);
    let align = 1usize << shift;

    let aligned = checked_align_offset(base, offset, align);
    if let Some(aligned) = aligned {
        let input_absolute = base
            .checked_add(offset)
            .expect("successful alignment has a representable input address");
        let absolute = base
            .checked_add(aligned)
            .expect("successful alignment has a representable absolute address");
        assert!(absolute >= base);
        assert!(absolute >= input_absolute);
        assert!(absolute - input_absolute < align);
        assert!(absolute & (align - 1) == 0);
    }
}
