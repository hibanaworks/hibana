//! Single arithmetic authority for resident and scratch layouts.

#[inline(always)]
pub(crate) const fn add(lhs: usize, rhs: usize) -> usize {
    match lhs.checked_add(rhs) {
        Some(value) => value,
        None => crate::invariant(),
    }
}

#[inline(always)]
pub(crate) const fn sub(lhs: usize, rhs: usize) -> usize {
    match lhs.checked_sub(rhs) {
        Some(value) => value,
        None => crate::invariant(),
    }
}

#[inline(always)]
pub(crate) const fn mul(lhs: usize, rhs: usize) -> usize {
    match lhs.checked_mul(rhs) {
        Some(value) => value,
        None => crate::invariant(),
    }
}

/// Number of `u32` words required to store one bit per item.
///
/// Division precedes the optional increment, so this remains exact over the
/// complete `usize` domain without forming `bits + 31`.
#[inline(always)]
pub(crate) const fn u32_word_count(bits: usize) -> usize {
    let width = u32::BITS as usize;
    let complete_words = bits / width;
    if bits.is_multiple_of(width) {
        complete_words
    } else {
        complete_words + 1
    }
}

#[inline(always)]
pub(crate) const fn checked_align_up(value: usize, align: usize) -> Option<usize> {
    if !align.is_power_of_two() {
        crate::invariant();
    }
    let mask = align - 1;
    match value.checked_add(mask) {
        Some(padded) => Some(padded & !mask),
        None => None,
    }
}

#[inline(always)]
pub(crate) const fn align_up(value: usize, align: usize) -> usize {
    match checked_align_up(value, align) {
        Some(value) => value,
        None => crate::invariant(),
    }
}

#[inline(always)]
pub(crate) const fn checked_align_offset(
    base: usize,
    offset: usize,
    align: usize,
) -> Option<usize> {
    let absolute = match base.checked_add(offset) {
        Some(absolute) => absolute,
        None => return None,
    };
    let aligned = match checked_align_up(absolute, align) {
        Some(aligned) => aligned,
        None => return None,
    };
    aligned.checked_sub(base)
}

#[inline(always)]
pub(crate) const fn align_offset(base: usize, offset: usize, align: usize) -> usize {
    match checked_align_offset(base, offset, align) {
        Some(offset) => offset,
        None => crate::invariant(),
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::{align_offset, align_up, checked_align_offset, checked_align_up, u32_word_count};

    #[test]
    fn alignment_is_minimal_and_absolute_offsets_include_the_base() {
        assert_eq!(align_up(5, 8), 8);
        assert_eq!(align_offset(3, 5, 8), 5);
        assert_eq!(checked_align_up(usize::MAX, 2), None);
        assert_eq!(checked_align_offset(usize::MAX, 1, 1), None);
    }

    #[test]
    #[should_panic]
    fn non_power_of_two_alignment_is_never_accepted() {
        let _ = checked_align_up(0, 3);
    }

    #[test]
    fn bit_word_count_is_exact_at_zero_boundaries_and_usize_max() {
        assert_eq!(u32_word_count(0), 0);
        assert_eq!(u32_word_count(1), 1);
        assert_eq!(u32_word_count(32), 1);
        assert_eq!(u32_word_count(33), 2);
        assert_eq!(
            u32_word_count(usize::MAX),
            (usize::MAX / u32::BITS as usize) + 1
        );
    }
}

#[cfg(kani)]
mod kani;
