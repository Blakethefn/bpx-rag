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

pub(super) fn is_sensitive_directory(value: &str) -> bool {
    SENSITIVE_DIRECTORIES
        .iter()
        .any(|directory| value.eq_ignore_ascii_case(directory))
}

pub(super) fn is_ssh_identity(normalized_name: &str) -> bool {
    SSH_IDENTITY_STEMS.iter().any(|stem| {
        if normalized_name == *stem {
            return true;
        }
        let Some(suffix) = normalized_name.strip_prefix(stem) else {
            return false;
        };
        suffix.starts_with('.') && !normalized_name.ends_with(".pub")
    })
}
