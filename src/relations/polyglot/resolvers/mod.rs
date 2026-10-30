pub(super) mod namespaces;
pub(super) mod paths;
pub(super) mod project;
pub(super) mod python;

pub(super) use namespaces::{jvm_targets, namespace_targets};
pub(super) use paths::add_file_candidates;
pub(super) use project::{dart_package_path, go_module_path, go_package_files};
pub(super) use python::module_path as python_module_path;
