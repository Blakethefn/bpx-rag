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

pub(super) fn is_sensitive_path(source_root: &Path, path: &Path) -> bool {
    is_sensitive_relative_path(path.strip_prefix(source_root).unwrap_or(path))
}

pub(super) fn is_sensitive_relative_path(path: &Path) -> bool {
    const SAFE_ENV_TEMPLATES: &[&str] = &["example", "sample", "template", "dist", "defaults"];
    const SENSITIVE_BASENAMES: &[&str] = &[
        ".envrc",
        ".git-credentials",
        ".netrc",
        ".npmrc",
        ".pypirc",
        "application_default_credentials.json",
        "credentials",
        "credentials.json",
        "credentials.yaml",
        "credentials.yml",
        "kubeconfig",
        "sa-key.json",
        "secret",
        "secret.json",
        "secret.yaml",
        "secret.yml",
        "secrets",
        "secrets.json",
        "secrets.yaml",
        "secrets.yml",
        "service-account-key.json",
        "service-account.json",
        "service_account.json",
        "token.json",
    ];
    const SENSITIVE_EXTENSIONS: &[&str] = &["env", "jks", "keystore", "key", "p12", "pem", "pfx"];

    if path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        is_sensitive_directory(&value)
            || ["credentials", "secrets"]
                .iter()
                .any(|name| value.eq_ignore_ascii_case(name))
    }) {
        return true;
    }

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let normalized_name = file_name.to_ascii_lowercase();
    if normalized_name == ".env" {
        return true;
    }
    if let Some(suffix) = normalized_name.strip_prefix(".env.") {
        let template_suffix = suffix.rsplit('.').next().unwrap_or_default();
        if !SAFE_ENV_TEMPLATES.contains(&template_suffix) {
            return true;
        }
    }
    let private_key_file = is_ssh_identity(&normalized_name);
    let extensionless_private_key = path.extension().is_none()
        && (normalized_name.contains("private_key")
            || normalized_name.contains("private-key")
            || normalized_name.contains("privatekey")
            || (normalized_name.starts_with("ssh_host_") && normalized_name.ends_with("_key")));
    if SENSITIVE_BASENAMES.contains(&normalized_name.as_str())
        || SENSITIVE_EXTENSIONS.iter().any(|extension| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|actual| actual.eq_ignore_ascii_case(extension))
        })
        || normalized_name.contains(".tfstate")
        || private_key_file
        || extensionless_private_key
    {
        return true;
    }
    false
}

pub(super) fn contains_google_api_key(content: &str) -> bool {
    const PREFIX: &[u8] = b"AIza";
    const KEY_LENGTH: usize = 39;
    let bytes = content.as_bytes();
    bytes
        .windows(PREFIX.len())
        .enumerate()
        .any(|(start, candidate)| {
            if candidate != PREFIX {
                return false;
            }
            let end = start + KEY_LENGTH;
            if end > bytes.len()
                || (start > 0 && is_google_api_key_byte(bytes[start - 1]))
                || (end < bytes.len() && is_google_api_key_byte(bytes[end]))
            {
                return false;
            }
            bytes[start + PREFIX.len()..end]
                .iter()
                .all(|byte| is_google_api_key_byte(*byte))
        })
}

pub(super) fn is_google_api_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

pub(super) fn is_sensitive_source_root(path: &Path) -> bool {
    let sensitive_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            ["credentials", "secrets"]
                .iter()
                .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
        });
    sensitive_name
        || path.components().any(|component| {
            let value = component.as_os_str().to_string_lossy();
            is_sensitive_directory(&value)
        })
}

fn is_sensitive_directory(value: &str) -> bool {
    SENSITIVE_DIRECTORIES
        .iter()
        .any(|directory| value.eq_ignore_ascii_case(directory))
}

fn is_ssh_identity(normalized_name: &str) -> bool {
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

#[cfg(test)]
mod tests;
