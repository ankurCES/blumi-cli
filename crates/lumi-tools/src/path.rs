//! Path resolution shared by the file tools.

use std::path::{Path, PathBuf};

/// Resolve a (possibly relative) tool path against the working directory.
pub fn resolve(working_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    }
}
