use super::BlobPtr;

static BYTES: [u8; 5] = [11, 22, 33, 44, 55];

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn static_blob_reads_are_bounded_by_the_used_prefix() {
    for len in 0..=BYTES.len() {
        let blob = BlobPtr::from_array(&BYTES, len);
        assert_eq!(blob.len(), len);
        for (index, expected) in BYTES.iter().enumerate().take(len) {
            assert_eq!(blob.byte_at(index), *expected);
        }
        for index in [len, BYTES.len(), usize::MAX] {
            assert!(std::panic::catch_unwind(|| blob.byte_at(index)).is_err());
        }
    }
    assert!(std::panic::catch_unwind(|| BlobPtr::from_array(&BYTES, BYTES.len() + 1)).is_err());
}

#[cfg(kani)]
fn out_of_prefix_read() -> (usize, usize) {
    let len = usize::from(kani::any::<u8>()) % (BYTES.len() + 1);
    (len, len.saturating_add(kani::any::<usize>()))
}

#[cfg(kani)]
#[kani::proof]
fn static_blob_preserves_every_valid_prefix_read() {
    let len = usize::from(kani::any::<u8>()) % (BYTES.len() + 1);
    let index: usize = kani::any();
    kani::cover!(len == 0);
    kani::cover!(len == BYTES.len());
    let blob = BlobPtr::from_array(&BYTES, len);
    assert_eq!(blob.len(), len);
    if index < len {
        assert_eq!(blob.byte_at(index), BYTES[index]);
    }
    // Prove the rejection constructor cannot fail before the tested read.
    let (len, index) = out_of_prefix_read();
    assert!(len <= BYTES.len());
    assert!(index >= len);
    kani::cover!(index == len);
    kani::cover!(index == usize::MAX);
}

#[cfg(kani)]
#[kani::proof]
#[kani::should_panic]
fn static_blob_rejects_every_read_outside_its_used_prefix() {
    let (len, index) = out_of_prefix_read();
    let blob = BlobPtr::from_array(&BYTES, len);
    let _ = blob.byte_at(index);
}
