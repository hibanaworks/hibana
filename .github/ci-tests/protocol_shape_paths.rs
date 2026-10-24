//! Keep the full arbitrary-precision rank while bounding filesystem names.
use num_bigint::BigUint;
use std::path::PathBuf;

pub(crate) fn relative(depth: usize, rank: &BigUint, count: usize) -> PathBuf {
    let mut path = PathBuf::from(format!("syntax-depth-{depth}"));
    // Each component stays below the 255-byte limit on the supported hosts.
    // No digest, truncation or collision-prone abbreviation replaces the rank.
    for chunk in rank.to_string().as_bytes().chunks(128) {
        path.push(format!("rank-{}", std::str::from_utf8(chunk).unwrap()));
    }
    path.push(format!("count-{count}"));
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_rank_names_are_bounded_and_retain_the_exact_identity() {
        let rank = BigUint::from(1u8) << 1790usize;
        let path = relative(8, &rank, 1);
        let mut digits = String::new();
        for part in path.components() {
            let text = part.as_os_str().to_str().unwrap();
            assert!(text.len() < 255);
            if let Some(chunk) = text.strip_prefix("rank-") {
                digits.push_str(chunk);
            }
        }
        assert_eq!(digits.parse::<BigUint>().unwrap(), rank);
        assert_ne!(path, relative(8, &(&rank + 1u8), 1));
        assert_ne!(path, relative(8, &rank, 2));
        assert_ne!(path, relative(7, &rank, 1));
    }
}
