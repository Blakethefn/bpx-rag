use super::super::is_sensitive_path;
use std::path::Path;

#[test]
fn sensitivity_uses_the_path_inside_the_source_root() {
    let root = Path::new("/data/project");
    assert!(is_sensitive_path(
        root,
        Path::new("/data/project/.ssh/id_rsa")
    ));
    assert!(!is_sensitive_path(
        root,
        Path::new("/data/project/src/main.rs")
    ));
    assert!(
        !is_sensitive_path(Path::new("/data/.ssh"), Path::new("/data/.ssh/config")),
        "components of the source root itself are not judged again"
    );
    assert!(is_sensitive_path(
        Path::new("/data/project"),
        Path::new("/other/.ssh/config")
    ));
}
