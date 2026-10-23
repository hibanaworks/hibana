use super::{read_word_pair, write_word_pair};

/// Check the codec used for epochs and credits over all stored/input bits.
/// Scope selection and reentry are covered by the cursor and Endpoint tests.
#[kani::proof]
#[kani::unwind(5)]
fn packed_roll_words_preserve_every_bit_and_the_adjacent_counter() {
    let initial = kani::any::<[u32; 4]>();
    let mut words = initial;
    let value = kani::any::<u64>();
    let target = usize::from(kani::any::<bool>());
    write_word_pair(words[target * 2..].first_chunk_mut().unwrap(), value);
    assert_eq!(
        read_word_pair(words[target * 2..].first_chunk().unwrap()),
        value,
    );
    let untouched = (1 - target) * 2;
    assert_eq!(words[untouched], initial[untouched]);
    assert_eq!(words[untouched + 1], initial[untouched + 1]);
    assert_eq!(
        read_word_pair(initial.first_chunk().unwrap()),
        u64::from(initial[0]) | (u64::from(initial[1]) << 32),
    );
}
