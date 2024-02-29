mod error;
mod icon2svg;

#[cfg(test)]
pub(crate) fn testdata_dir() -> std::path::PathBuf {
    use std::{path::PathBuf, str::FromStr};
    PathBuf::from_str("resources/testdata").unwrap()
}

#[cfg(test)]
pub(crate) fn testdata_string(path: &str) -> String {
    use std::fs;

    let path = testdata_dir().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Unable to read {path:?}: {e}"))
}

#[cfg(test)]
pub(crate) fn testdata_bytes(path: &str) -> Vec<u8> {
    use std::fs;

    let path = testdata_dir().join(path);
    fs::read(&path).unwrap_or_else(|e| panic!("Unable to read {path:?}: {e}"))
}
