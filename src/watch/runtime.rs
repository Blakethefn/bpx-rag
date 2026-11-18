use std::io;

pub(super) fn permission_denied(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<io::Error>().is_some_and(is_denied)
            || cause.downcast_ref::<notify::Error>().is_some_and(
                |error| matches!(&error.kind, notify::ErrorKind::Io(error) if is_denied(error)),
            )
    })
}

fn is_denied(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
    )
}

#[cfg(test)]
mod tests;
