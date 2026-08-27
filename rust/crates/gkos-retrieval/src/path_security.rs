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
    if windows_path_has_reparse_component(path).ok()? {
        return None;
    }
    let expanded = expand_windows_short_names(path).unwrap_or_else(|| path.to_path_buf());
    let value = expanded.as_os_str().to_str()?.replace('/', "\\");
    let value = value
        .strip_prefix(r"\\?\UNC\")
        .map(|suffix| format!(r"\\{suffix}"))
        .or_else(|| value.strip_prefix(r"\\?\").map(ToOwned::to_owned))
        .unwrap_or(value);
    Some(value.trim_end_matches('\\').to_lowercase())
}

/// Expand only Windows 8.3 short-name components. Reparse-point components are
/// rejected separately before this expansion, so the later comparison cannot
/// mistake a junction or symlink target for the caller's original path.
/// GitHub-hosted Windows runners expose their ordinary temporary directory as
/// `RUNNER~1`; rejecting that spelling would make every legitimate state path
/// below the runner temp directory unusable.
#[cfg(windows)]
fn expand_windows_short_names(path: &Path) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr;
    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;

    let input = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let required = unsafe { GetLongPathNameW(input.as_ptr(), ptr::null_mut(), 0) };
    if required == 0 {
        return None;
    }
    let mut output = vec![0_u16; required as usize];
    let written = unsafe {
        GetLongPathNameW(
            input.as_ptr(),
            output.as_mut_ptr(),
            output.len().try_into().ok()?,
        )
    };
    if written == 0 || written as usize >= output.len() {
        return None;
    }
    output.truncate(written as usize);
    Some(PathBuf::from(OsString::from_wide(&output)))
}

#[cfg(windows)]
fn windows_path_has_reparse_component(path: &Path) -> io::Result<bool> {
    use std::fs;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let absolute = absolute_lexical_path(path)?;
    let mut candidate = PathBuf::new();
    for component in absolute.components() {
        candidate.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) => {
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Ok(true);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(false)
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

    #[cfg(windows)]
    #[test]
    fn ordinary_existing_windows_temp_paths_match_their_realpath_spelling() {
        let directory = tempfile::tempdir().unwrap();
        let lexical = absolute_lexical_path(directory.path()).unwrap();
        let canonical = std::fs::canonicalize(&lexical).unwrap();
        assert!(
            equivalent_paths(&lexical, &canonical),
            "ordinary temp path {lexical:?} must match its realpath {canonical:?}"
        );
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
