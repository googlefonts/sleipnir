use std::fmt::Debug;
use std::iter::FromIterator;
use std::path::PathBuf;

#[track_caller]
pub fn assert_file_eq_impl<T: AsRef<[u8]>>(actual_bytes: T, file: &str) {
    let actual_bytes = actual_bytes.as_ref();
    let expected_path = PathBuf::from_iter(["resources/testdata", file]);
    let expected_bytes = std::fs::read(&expected_path)
        .inspect_err(|err| eprintln!("Failed to read {expected_path:?}: {err}"))
        .unwrap_or_default();

    let actual_dir = "target/testdata";
    if let Err(err) = std::fs::create_dir_all(actual_dir) {
        eprintln!("Failed to create target/testdata directory: {err}");
    }
    let actual_path = PathBuf::from_iter([actual_dir, file]);
    if let Err(err) = std::fs::write(&actual_path, actual_bytes) {
        eprintln!("Failed to write actual bytes to {actual_path:?}: {err}");
    }

    assert!(
        actual_bytes == expected_bytes,
        "Bytes (stored in {actual_path:?}) did not match expected bytes from {expected_path:?}"
    );
}

/// Asserts that the given bytes match the contents of a file in `resources/testdata`.
/// Also writes the actual bytes to `target/testdata` for easy diffing.
#[macro_export]
macro_rules! assert_file_eq {
    ($actual:expr, $expected_file:expr) => {
        $crate::test_utils::assert_file_eq_impl(&$actual, $expected_file);
    };
}

/// Asserts that an expression matches a pattern.
///
/// ## Example
///
/// ```
/// let result = Ok(1);
/// assert_matches!(result, Ok(_));
/// ```
///
/// TODO: Remove this macro once std::assert_matches! is stable.
/// See: https://doc.rust-lang.org/std/assert_matches/macro.assert_matches.html
#[macro_export]
macro_rules! assert_matches {
    ($expression:expr, $pattern:pat) => {
        // We allow redundant pattern matching since the debug output is sometimes more useful. We
        // want "Got <error> and expected .." instead of "failed val.is_ok()".
        #[allow(clippy::redundant_pattern_matching)]
        if !(matches!($expression, $pattern)) {
            let res = $expression;
            panic!(
                "assertion failed: {expr} result {res:?} does not match {pattern}",
                expr = stringify!($expression),
                pattern = stringify!($pattern)
            );
        }
    };
}

/// Asserts that two vectors contain the same elements, ignoring order.
#[track_caller]
pub fn assert_vec_unordered_eq_impl<T: PartialEq + Debug>(actual: &[T], expected: &[T]) {
    let is_eq =
        actual.len() == expected.len() && actual.iter().all(|actual| expected.contains(actual));
    assert!(
        is_eq,
        "Vectors do not contain the same elements. \nActual: {:?}\nExpected: {:?}",
        actual, expected
    );
    assert_eq!(actual.len(),
               expected.len(),
               "Vectors have different lengths, but contain the same unique elements. This suggests duplicates. \nActual: {:?}\nExpected: {:?}",
               actual,
               expected);
}

/// Asserts that two vectors contain the same elements, ignoring order.
#[macro_export]
macro_rules! assert_vec_unordered_eq {
    ($actual:expr, $expected:expr) => {
        $crate::test_utils::assert_vec_unordered_eq_impl(&$actual, &$expected);
    };
}
