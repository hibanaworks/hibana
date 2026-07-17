use super::{CodecError, Payload, WireEncode, WirePayload, fixed_array_schema_id};

#[kani::proof]
fn fixed_array_schema_identity_is_injective_over_the_complete_admitted_domain() {
    let left: u32 = kani::any();
    let right: u32 = kani::any();
    kani::assume(left <= 0x00ff_ffff);
    kani::assume(right <= 0x00ff_ffff);

    let left_schema = fixed_array_schema_id(left as usize);
    let right_schema = fixed_array_schema_id(right as usize);
    assert_eq!(left_schema & 0x00ff_ffff, left);
    assert_eq!(right_schema & 0x00ff_ffff, right);
    assert!(left_schema != right_schema || left == right);
}

#[kani::proof]
#[kani::should_panic]
fn fixed_array_schema_identity_rejects_the_first_colliding_width() {
    let _ = fixed_array_schema_id(0x0100_0000);
}

macro_rules! check_integer_codec {
    ($ty:ty, $width:expr, $schema:expr) => {{
        let value = kani::any::<$ty>();
        let mut encoded = [0u8; $width];
        assert_eq!(value.encode_into(&mut encoded), Ok($width));
        assert_eq!(encoded, value.to_be_bytes());
        assert_eq!(<$ty as WirePayload>::SCHEMA_ID, $schema);

        let payload = Payload::new(&encoded);
        assert_eq!(<$ty as WirePayload>::validate_payload(payload), Ok(()));
        assert_eq!(
            <$ty as WirePayload>::decode_validated_payload(payload),
            value
        );

        let storage = [0u8; $width + 1];
        let len = usize::from(kani::any::<u8>() % (($width + 2) as u8));
        let result = <$ty as WirePayload>::validate_payload(Payload::new(&storage[..len]));
        let expected = match len.cmp(&$width) {
            core::cmp::Ordering::Less => Err(CodecError::Truncated),
            core::cmp::Ordering::Equal => Ok(()),
            core::cmp::Ordering::Greater => Err(CodecError::Malformed),
        };
        assert_eq!(result, expected);

        let mut output = [0u8; $width + 1];
        let capacity = usize::from(kani::any::<u8>() % (($width + 2) as u8));
        let encoded = value.encode_into(&mut output[..capacity]);
        if capacity < $width {
            assert_eq!(encoded, Err(CodecError::Truncated));
        } else {
            assert_eq!(encoded, Ok($width));
            assert_eq!(&output[..$width], &value.to_be_bytes());
        }
    }};
}

#[kani::proof]
fn builtin_u8_i8_codecs_are_exact() {
    check_integer_codec!(u8, 1, 2);
    check_integer_codec!(i8, 1, 3);
}

#[kani::proof]
fn builtin_u16_i16_codecs_are_exact() {
    check_integer_codec!(u16, 2, 4);
    check_integer_codec!(i16, 2, 5);
}

#[kani::proof]
fn builtin_u32_i32_codecs_are_exact() {
    check_integer_codec!(u32, 4, 6);
    check_integer_codec!(i32, 4, 7);
}

#[kani::proof]
fn builtin_u64_i64_codecs_are_exact() {
    check_integer_codec!(u64, 8, 8);
    check_integer_codec!(i64, 8, 9);
}

#[kani::proof]
fn builtin_u128_i128_codecs_are_exact() {
    check_integer_codec!(u128, 16, 10);
    check_integer_codec!(i128, 16, 11);
}

#[kani::proof]
fn builtin_bool_codec_accepts_exact_canonical_bytes() {
    let value = kani::any::<bool>();
    let mut encoded = [0u8; 1];
    assert_eq!(value.encode_into(&mut encoded), Ok(1));
    assert_eq!(encoded[0], u8::from(value));
    assert_eq!(<bool as WirePayload>::SCHEMA_ID, 1);

    let payload = Payload::new(&encoded);
    assert_eq!(<bool as WirePayload>::validate_payload(payload), Ok(()));
    assert_eq!(
        <bool as WirePayload>::decode_validated_payload(payload),
        value
    );
    let mut empty = [];
    assert_eq!(value.encode_into(&mut empty), Err(CodecError::Truncated));

    let storage = [kani::any::<u8>(), kani::any::<u8>()];
    let len = usize::from(kani::any::<u8>() % 3);
    let result = <bool as WirePayload>::validate_payload(Payload::new(&storage[..len]));
    let expected = match len {
        0 => Err(CodecError::Truncated),
        1 if storage[0] <= 1 => Ok(()),
        _ => Err(CodecError::Malformed),
    };
    assert_eq!(result, expected);
}

#[kani::proof]
fn builtin_unit_codec_is_exact() {
    let mut unit_output = [kani::any::<u8>(), kani::any::<u8>()];
    let original = unit_output;
    assert_eq!(().encode_into(&mut unit_output), Ok(0));
    assert_eq!(unit_output, original);
    assert_eq!(<() as WirePayload>::SCHEMA_ID, 0);
    assert_eq!(
        <() as WirePayload>::decode_validated_payload(Payload::new(&[])),
        ()
    );

    let unit_storage = [kani::any::<u8>(), kani::any::<u8>()];
    let unit_len = usize::from(kani::any::<u8>() % 3);
    let unit_result =
        <() as WirePayload>::validate_payload(Payload::new(&unit_storage[..unit_len]));
    assert_eq!(
        unit_result,
        if unit_len == 0 {
            Ok(())
        } else {
            Err(CodecError::Malformed)
        }
    );
}

fn check_borrowed_bytes_roundtrip(value: &[u8]) {
    let len = value.len();
    let mut encoded = [0u8; 4];
    assert_eq!(value.encode_into(&mut encoded), Ok(len));
    assert_eq!(&encoded[..len], value);
    assert_eq!(<&[u8] as WirePayload>::SCHEMA_ID, 12);
    let payload = Payload::new(value);
    assert_eq!(<&[u8] as WirePayload>::validate_payload(payload), Ok(()));
    assert_eq!(
        <&[u8] as WirePayload>::decode_validated_payload(payload),
        value
    );
}

#[kani::proof]
fn builtin_borrowed_bytes_roundtrip_is_exact() {
    let bytes = kani::any::<[u8; 4]>();
    match kani::any::<u8>() % 5 {
        0 => check_borrowed_bytes_roundtrip(&bytes[..0]),
        1 => check_borrowed_bytes_roundtrip(&bytes[..1]),
        2 => check_borrowed_bytes_roundtrip(&bytes[..2]),
        3 => check_borrowed_bytes_roundtrip(&bytes[..3]),
        _ => check_borrowed_bytes_roundtrip(&bytes),
    }
}

#[kani::proof]
fn builtin_borrowed_bytes_truncation_is_exact() {
    let bytes = kani::any::<[u8; 4]>();
    let value = bytes.as_slice();
    let capacity = usize::from(kani::any::<u8>() % 5);
    let mut bounded = [0u8; 4];
    let result = value.encode_into(&mut bounded[..capacity]);
    if capacity < value.len() {
        assert_eq!(result, Err(CodecError::Truncated));
    } else {
        assert_eq!(result, Ok(value.len()));
        assert_eq!(bounded.as_slice(), value);
    }
}

macro_rules! check_fixed_array_codec {
    ($width:expr) => {{
        let value = kani::any::<[u8; $width]>();
        let mut encoded = [0u8; $width];
        assert_eq!(value.encode_into(&mut encoded), Ok($width));
        assert_eq!(encoded, value);
        assert_eq!(
            <[u8; $width] as WirePayload>::SCHEMA_ID,
            0x0100_0000 | $width
        );
        let payload = Payload::new(&encoded);
        assert_eq!(
            <[u8; $width] as WirePayload>::validate_payload(payload),
            Ok(())
        );
        assert_eq!(
            <[u8; $width] as WirePayload>::decode_validated_payload(payload),
            value
        );

        let storage = [0u8; $width + 1];
        let len = usize::from(kani::any::<u8>() % (($width + 2) as u8));
        let result = <[u8; $width] as WirePayload>::validate_payload(Payload::new(&storage[..len]));
        let expected = match len.cmp(&$width) {
            core::cmp::Ordering::Less => Err(CodecError::Truncated),
            core::cmp::Ordering::Equal => Ok(()),
            core::cmp::Ordering::Greater => Err(CodecError::Malformed),
        };
        assert_eq!(result, expected);

        let capacity = usize::from(kani::any::<u8>() % (($width + 2) as u8));
        let mut output = [0u8; $width + 1];
        let encoded = value.encode_into(&mut output[..capacity]);
        match capacity.cmp(&$width) {
            core::cmp::Ordering::Less => {
                assert_eq!(encoded, Err(CodecError::Truncated));
            }
            core::cmp::Ordering::Equal | core::cmp::Ordering::Greater => {
                assert_eq!(encoded, Ok($width));
                assert_eq!(&output[..$width], value.as_slice());
            }
        }
    }};
}

#[kani::proof]
fn builtin_fixed_array_schema_and_bytes_are_exact() {
    check_fixed_array_codec!(0);
    check_fixed_array_codec!(1);
    check_fixed_array_codec!(4);
}
