use std::io;
use std::path::{Component, Path, PathBuf};

/// Resolve a caller spelling against the current directory and remove only
/// lexical `.`/`..` components. This deliberately does not follow links; the
/// caller can then compare the normalized spelling with `canonicalize` to
/// detect symlink/junction aliases without rejecting ordinary relative paths.
pub(crate) fn absolute_lexical_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "path escapes its absolute root",
                    ));
                }
            }
        }
    }
    if !normalized.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path did not resolve to an absolute spelling",
        ));
    }
    Ok(normalized)
}

/// Compare absolute paths without treating Windows' extended-length prefix or
/// case normalization as an alias. Callers still canonicalize and reject
/// symlinks/hardlinks before using this helper.
#[cfg(windows)]
pub(crate) fn equivalent_paths(left: &Path, right: &Path) -> bool {
    matches!(
        (windows_key(left), windows_key(right)),
        (Some(left), Some(right)) if left == right
    )
}

#[cfg(not(windows))]
pub(crate) fn equivalent_paths(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
pub(crate) fn contained_path(path: &Path, root: &Path) -> bool {
    let (Some(path), Some(root)) = (windows_key(path), windows_key(root)) else {
        return false;
    };
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('\\'))
}

#[cfg(not(windows))]
pub(crate) fn contained_path(path: &Path, root: &Path) -> bool {
    path == root || path.strip_prefix(root).is_ok()
}

#[cfg(windows)]
fn windows_key(path: &Path) -> Option<String> {
    let value = path.as_os_str().to_str()?.replace('/', "\\");
    let value = value
        .strip_prefix(r"\\?\UNC\")
        .map(|suffix| format!(r"\\{suffix}"))
        .or_else(|| value.strip_prefix(r"\\?\").map(ToOwned::to_owned))
        .unwrap_or(value);
    Some(value.trim_end_matches('\\').to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_absolute_path_removes_dot_components_but_not_aliases() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            absolute_lexical_path(Path::new("alpha/./beta/../gamma")).unwrap(),
            cwd.join("alpha").join("gamma")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefix_and_case_normalization_preserve_containment_boundaries() {
        let ordinary = Path::new(r"C:\Vault\State");
        let extended = Path::new(r"\\?\c:\vault\state");
        assert!(equivalent_paths(ordinary, extended));
        assert!(contained_path(
            Path::new(r"\\?\C:\VAULT\state\generation.sqlite"),
            ordinary
        ));
        assert!(!contained_path(Path::new(r"C:\Vault2\file"), ordinary));
    }

    #[cfg(not(windows))]
    #[test]
    fn posix_paths_use_exact_component_containment() {
        assert!(equivalent_paths(Path::new("/vault"), Path::new("/vault")));
        assert!(contained_path(
            Path::new("/vault/file"),
            Path::new("/vault")
        ));
        assert!(!contained_path(
            Path::new("/vault2/file"),
            Path::new("/vault")
        ));
    }
}
