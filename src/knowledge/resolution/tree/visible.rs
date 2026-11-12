use std::collections::{HashSet, VecDeque};

use super::{path_segments, walk_modules, Symbol, Walk};

impl super::Tree {
    pub(crate) fn is_method(&self, symbol: &Symbol) -> bool {
        let Some(container) = symbol.container.as_deref() else {
            return false;
        };
        if self
            .inline_names
            .get(&symbol.file_id)
            .is_some_and(|names| names.contains(container))
        {
            return false;
        }
        matches!(
            symbol.kind.as_str(),
            "function" | "const" | "type" | "static"
        )
    }

    pub(crate) fn visible(&self, symbol: &Symbol, from: i64) -> bool {
        if symbol.file_id == from {
            return true;
        }
        let Some(visibility) = symbol.visibility.as_deref() else {
            return false;
        };
        match visibility {
            "pub" => true,
            "pub(crate)" => self.same_crate(symbol.file_id, from),
            "private" | "pub(self)" => self.descendants(symbol.file_id).contains(&from),
            "pub(super)" => self.pub_super_visible(symbol.file_id, from),
            other if other.starts_with("pub(in ") && other.ends_with(')') => {
                let path = &other["pub(in ".len()..other.len() - 1];
                self.pub_in_visible(symbol.file_id, from, path)
            }
            _ => false,
        }
    }

    pub(crate) fn glob_visible(&self, symbol: &Symbol, glob_file: i64, module_file: i64) -> bool {
        if symbol.file_id != module_file || self.is_method(symbol) {
            return false;
        }
        let Some(visibility) = symbol.visibility.as_deref() else {
            return false;
        };
        match visibility {
            "pub" => true,
            "pub(crate)" => self.same_crate(glob_file, module_file),
            "pub(super)" => self.super_module(module_file) == Some(glob_file),
            other if other == "pub(in crate)" || other.starts_with("pub(in crate::") => {
                self.same_crate(glob_file, module_file)
            }
            other if other.starts_with("pub(in ") && other.ends_with(')') => {
                let path = &other["pub(in ".len()..other.len() - 1];
                if path == "super" {
                    self.super_module(module_file) == Some(glob_file)
                } else if path == "self" {
                    false
                } else {
                    self.module_file(module_file, path) == Some(glob_file)
                }
            }
            _ => false,
        }
    }

    pub(crate) fn module_file(&self, from: i64, path: &str) -> Option<i64> {
        let segments = path_segments(path)?;
        match walk_modules(self, from, &segments) {
            Walk::File(file) => Some(file),
            Walk::External | Walk::Failed => None,
        }
    }

    pub(crate) fn limitation_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.has_macro {
            lines.push("limitation:macros_not_expanded".to_string());
        }
        if self.has_inline_module {
            lines.push("limitation:inline_modules".to_string());
        }
        if self.withheld_public_api() {
            lines.push("limitation:public_api".to_string());
        }
        lines
    }

    fn withheld_public_api(&self) -> bool {
        self.symbols.iter().any(|symbol| {
            self.is_rust_file(symbol.file_id)
                && symbol.visibility.as_deref() == Some("pub")
                && self.library_files.contains(&symbol.file_id)
        })
    }

    fn pub_super_visible(&self, item_file: i64, from: i64) -> bool {
        match self.super_module(item_file) {
            Some(parent) => self.descendants(parent).contains(&from),
            None => self.descendants(item_file).contains(&from),
        }
    }

    fn pub_in_visible(&self, item_file: i64, from: i64, path: &str) -> bool {
        if path == "self" {
            return self.descendants(item_file).contains(&from);
        }
        if path == "super" {
            return self.pub_super_visible(item_file, from);
        }
        match self.module_file(item_file, path) {
            Some(allowed) => self.descendants(allowed).contains(&from),
            None => false,
        }
    }

    fn descendants(&self, root: i64) -> HashSet<i64> {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([root]);
        while let Some(file) = queue.pop_front() {
            if !seen.insert(file) {
                continue;
            }
            if let Some(children) = self.children.get(&file) {
                queue.extend(children.values().copied());
            }
        }
        seen
    }
}
