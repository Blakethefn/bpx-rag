use super::*;

#[test]
fn permission_denied_is_classified_through_the_error_chain() {
    let error = anyhow::Error::new(io::Error::from(io::ErrorKind::PermissionDenied));
    assert!(permission_denied(&error));
    let read_only = anyhow::Error::new(io::Error::from(io::ErrorKind::ReadOnlyFilesystem));
    assert!(permission_denied(&read_only));
    let other = anyhow::Error::new(io::Error::from(io::ErrorKind::NotFound));
    assert!(!permission_denied(&other));
}
