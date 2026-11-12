use super::{is_sensitive_path, is_sensitive_relative_path, is_sensitive_source_root};
use std::path::Path;

const SENSITIVE_DIRECTORIES: &[&str] = &[
    ".aws",
    ".azure",
    ".git",
    ".gnupg",
    ".hg",
    ".kube",
    ".ssh",
    ".svn",
    ".terraform",
];

const SSH_IDENTITY_STEMS: &[&str] = &["id_dsa", "id_ecdsa", "id_ed25519", "id_rsa", "id_xmss"];

#[test]
fn sensitive_directories_reject_nested_files_and_source_roots() {
    for directory in SENSITIVE_DIRECTORIES {
        let nested = Path::new(directory).join("config");
        assert!(
            is_sensitive_relative_path(&nested),
            "{directory} hides files inside it"
        );
        let folded = directory.to_ascii_uppercase();
        assert!(
            is_sensitive_relative_path(&Path::new(&folded).join("config")),
            "{folded} matches regardless of case"
        );
        assert!(
            is_sensitive_relative_path(&Path::new("src").join(directory).join("config")),
            "{directory} matches as a middle component"
        );

        assert!(
            is_sensitive_source_root(Path::new(directory)),
            "{directory} is a sensitive source root"
        );
        assert!(
            is_sensitive_source_root(&Path::new("/srv").join(directory).join("project")),
            "{directory} rejects a source root that contains it"
        );
        assert!(
            is_sensitive_source_root(&Path::new("/srv").join(&folded)),
            "{folded} rejects a source root regardless of case"
        );
    }
}

#[test]
fn credential_and_secret_names_stay_off_the_directory_catalog() {
    for name in ["credentials", "secrets"] {
        assert!(
            is_sensitive_relative_path(&Path::new(name).join("token.txt")),
            "{name} as a directory still hides its files"
        );
        assert!(
            is_sensitive_relative_path(Path::new(name)),
            "{name} as a file is still sensitive"
        );
        let folded = name.to_ascii_uppercase();
        assert!(is_sensitive_relative_path(Path::new(&folded)));

        assert!(
            is_sensitive_source_root(Path::new(name)),
            "{name} is sensitive when it is the source root basename"
        );
        assert!(is_sensitive_source_root(&Path::new("/var").join(&folded)));
        assert!(
            !is_sensitive_source_root(&Path::new("/var").join(name).join("project")),
            "{name} as an ancestor does not make another root sensitive"
        );
    }

    for path in [
        "secrets.txt",
        "credentials.bak",
        "my-credentials",
        "src/main.rs",
    ] {
        assert!(
            !is_sensitive_relative_path(Path::new(path)),
            "{path} is not a credential basename"
        );
    }
    assert!(!is_sensitive_source_root(Path::new("/var/project")));
}

#[test]
fn ssh_identity_stems_reject_the_key_and_dotted_suffixes_except_pub() {
    for stem in SSH_IDENTITY_STEMS {
        assert!(
            is_sensitive_relative_path(Path::new(stem)),
            "{stem} itself is a private key"
        );
        assert!(is_sensitive_relative_path(Path::new(
            &stem.to_ascii_uppercase()
        )));
        assert!(is_sensitive_relative_path(
            &Path::new("keys").join(format!("{stem}.pem"))
        ));
        assert!(is_sensitive_relative_path(Path::new(&format!(
            "{stem}.bak"
        ))));
        assert!(is_sensitive_relative_path(Path::new(&format!(
            "{stem}.public"
        ))));

        assert!(
            !is_sensitive_relative_path(Path::new(&format!("{stem}.pub"))),
            "{stem}.pub stays indexable"
        );
        assert!(!is_sensitive_relative_path(Path::new(&format!(
            "{stem}.PUB"
        ))));
        assert!(!is_sensitive_relative_path(Path::new(&format!(
            "{stem}.something.pub"
        ))));
        assert!(!is_sensitive_relative_path(Path::new(&format!(
            "{stem}bak"
        ))));
        assert!(!is_sensitive_relative_path(Path::new(&format!("x{stem}"))));
    }
}

#[test]
fn names_that_only_share_a_prefix_stay_indexable() {
    for path in [
        ".gitignore",
        ".github/workflows/ci.yml",
        ".gitmodules",
        ".terraformrc",
        "notes.md",
        "src/main.rs",
    ] {
        assert!(
            !is_sensitive_relative_path(Path::new(path)),
            "{path} is outside the sensitive catalogs"
        );
    }
}

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
