//! In-memory Rust module tree for name resolution.
//!
//! Package boundaries, `mod` edges, and crate roots live here. They are not written onto
//! `module` or `path` resolution rows. Reachability keeps reading those rows.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) struct Symbol {
    pub id: i64,
    pub file_id: i64,
    pub name: String,
    pub kind: String,
    pub container: Option<String>,
    pub visibility: Option<String>,
}

struct ModRef {
    file_id: i64,
    from_rel: String,
    name: String,
    qualifier: Option<String>,
}

pub(crate) struct Tree {
    symbols: Vec<Symbol>,
    symbols_by_id: HashMap<i64, usize>,
    symbols_by_file: HashMap<i64, Vec<usize>>,
    rust_files: HashSet<i64>,
    children: HashMap<i64, HashMap<String, i64>>,
    parents: HashMap<i64, Vec<i64>>,
    /// Crate root for a file that has exactly one. A root maps to itself.
    crate_of: HashMap<i64, i64>,
    roots: HashSet<i64>,
    library_files: HashSet<i64>,
    program_files: HashSet<i64>,
    program_entries: HashSet<i64>,
    test_names: HashMap<i64, HashSet<String>>,
    inline_names: HashMap<i64, HashSet<String>>,
    has_macro: bool,
    has_inline_module: bool,
}

pub(crate) fn is_rust_path(path: &str) -> bool {
    extension(path).eq_ignore_ascii_case("rs")
}

pub(crate) fn ident(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}

impl Tree {
    pub(crate) fn load(conn: &Connection, source_id: i64) -> Result<Self> {
        let files = load_files(conn, source_id)?;
        let symbols = load_symbols(conn, source_id)?;
        let modules = load_modules(conn, source_id)?;
        let entries = load_entries(conn, source_id)?;
        Ok(Self::build(files, symbols, modules, entries))
    }

    fn build(
        files: Vec<(i64, String)>,
        symbols: Vec<Symbol>,
        modules: Vec<ModRef>,
        entries: Vec<(Option<i64>, String, String)>,
    ) -> Self {
        let files_by_id: HashMap<i64, String> = files.into_iter().collect();
        let files_by_path: HashMap<&str, i64> = files_by_id
            .iter()
            .map(|(id, path)| (path.as_str(), *id))
            .collect();
        let cargo_dirs = cargo_directories(&files_by_id);
        let packages: HashMap<i64, Option<String>> = files_by_id
            .iter()
            .map(|(id, path)| (*id, package_of(path, &cargo_dirs)))
            .collect();
        let rust_files: HashSet<i64> = files_by_id
            .iter()
            .filter(|(_, path)| is_rust_path(path))
            .map(|(id, _)| *id)
            .collect();

        let mut symbols_by_id = HashMap::new();
        let mut symbols_by_file: HashMap<i64, Vec<usize>> = HashMap::new();
        for (index, symbol) in symbols.iter().enumerate() {
            symbols_by_id.insert(symbol.id, index);
            symbols_by_file
                .entry(symbol.file_id)
                .or_default()
                .push(index);
        }
        let has_macro = symbols.iter().any(|symbol| symbol.kind == "macro");

        let (children, parents) = module_tree(&modules, &files_by_path, &packages, &rust_files);
        let (library_roots, program_roots, program_entries, test_names) =
            roots(&entries, &rust_files);
        let root_ids: HashSet<i64> = library_roots
            .iter()
            .copied()
            .chain(program_roots.iter().copied())
            .collect();
        let library_files = reachable(&library_roots, &children);
        let program_files = reachable(&program_roots, &children);
        let crate_of = assign_crates(&root_ids, &children);
        let (inline_names, has_inline_module) = inline_modules(&symbols, &modules, &rust_files);

        Self {
            symbols,
            symbols_by_id,
            symbols_by_file,
            rust_files,
            children,
            parents,
            crate_of,
            roots: root_ids,
            library_files,
            program_files,
            program_entries,
            test_names,
            inline_names,
            has_macro,
            has_inline_module,
        }
    }

    pub(crate) fn is_rust_file(&self, file_id: i64) -> bool {
        self.rust_files.contains(&file_id)
    }

    pub(crate) fn symbol(&self, id: i64) -> &Symbol {
        &self.symbols[self.symbols_by_id[&id]]
    }

    pub(crate) fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    pub(crate) fn symbols_in(&self, file_id: i64) -> impl Iterator<Item = &Symbol> {
        self.symbols_by_file
            .get(&file_id)
            .into_iter()
            .flat_map(|indexes| indexes.iter().map(|&index| &self.symbols[index]))
    }

    pub(crate) fn child(&self, file_id: i64, name: &str) -> Option<i64> {
        self.children.get(&file_id)?.get(name).copied()
    }

    pub(crate) fn super_module(&self, file_id: i64) -> Option<i64> {
        if self.roots.contains(&file_id) {
            return None;
        }
        let parents = self.parents.get(&file_id)?;
        if parents.len() == 1 {
            Some(parents[0])
        } else {
            None
        }
    }

    pub(crate) fn crate_root(&self, file_id: i64) -> Option<i64> {
        self.crate_of.get(&file_id).copied()
    }

    pub(super) fn parents_of(&self, file_id: i64) -> &[i64] {
        self.parents.get(&file_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub(super) fn child_name(&self, parent: i64, child: i64) -> Option<&str> {
        self.children
            .get(&parent)?
            .iter()
            .find_map(|(name, id)| (*id == child).then_some(name.as_str()))
    }

    pub(crate) fn same_crate(&self, left: i64, right: i64) -> bool {
        match (self.crate_of.get(&left), self.crate_of.get(&right)) {
            (Some(left_root), Some(right_root)) => left_root == right_root,
            _ => false,
        }
    }

    pub(crate) fn on_program_only(&self, file_id: i64) -> bool {
        self.program_files.contains(&file_id) && !self.library_files.contains(&file_id)
    }

    pub(crate) fn is_program_main(&self, symbol: &Symbol) -> bool {
        ident(&symbol.name) == "main"
            && symbol.kind == "function"
            && !self.is_method(symbol)
            && self.program_entries.contains(&symbol.file_id)
    }

    pub(crate) fn is_named_test(&self, symbol: &Symbol) -> bool {
        symbol.kind == "function"
            && self
                .test_names
                .get(&symbol.file_id)
                .is_some_and(|names| names.contains(ident(&symbol.name)))
    }

    /// A function, const, or associated item whose container is a type or trait.
    /// Items inside an inline `mod` share the file module, so their container is not a method.
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

    /// Glob imports are narrower than path visibility. Private and `pub(self)` items stay out.
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
                && !self.on_program_only(symbol.file_id)
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

fn module_tree(
    modules: &[ModRef],
    files_by_path: &HashMap<&str, i64>,
    packages: &HashMap<i64, Option<String>>,
    rust_files: &HashSet<i64>,
) -> (HashMap<i64, HashMap<String, i64>>, HashMap<i64, Vec<i64>>) {
    let mut grouped: HashMap<(i64, &str), Vec<&ModRef>> = HashMap::new();
    for module in modules {
        if !rust_files.contains(&module.file_id) {
            continue;
        }
        grouped
            .entry((module.file_id, module.name.as_str()))
            .or_default()
            .push(module);
    }

    let mut children: HashMap<i64, HashMap<String, i64>> = HashMap::new();
    let mut parents: HashMap<i64, Vec<i64>> = HashMap::new();
    for ((file_id, name), refs) in grouped {
        let mut targets = HashSet::new();
        let mut blocked = false;
        for module in refs {
            match module_targets(module, files_by_path, packages, rust_files) {
                Targets::None => {}
                Targets::One(target) => {
                    targets.insert(target);
                }
                Targets::Blocked => blocked = true,
            }
        }
        if blocked || targets.len() != 1 {
            continue;
        }
        let Some(target) = targets.into_iter().next() else {
            continue;
        };
        children
            .entry(file_id)
            .or_default()
            .insert(name.to_string(), target);
        let listed = parents.entry(target).or_default();
        if !listed.contains(&file_id) {
            listed.push(file_id);
        }
    }
    (children, parents)
}

enum Targets {
    None,
    One(i64),
    Blocked,
}

fn module_targets(
    module: &ModRef,
    files_by_path: &HashMap<&str, i64>,
    packages: &HashMap<i64, Option<String>>,
    rust_files: &HashSet<i64>,
) -> Targets {
    let package = packages.get(&module.file_id);
    let accept = |rel: String| -> Option<i64> {
        let id = *files_by_path.get(rel.as_str())?;
        if id == module.file_id || !rust_files.contains(&id) {
            return None;
        }
        (packages.get(&id) == package).then_some(id)
    };
    if let Some(path) = module.qualifier.as_deref().filter(|path| !path.is_empty()) {
        let rel = normalize_rel(&join_rel(&parent_dir(&module.from_rel), path));
        return match accept(rel) {
            Some(id) => Targets::One(id),
            None => Targets::None,
        };
    }
    let base = module_base(&module.from_rel);
    let mut hits = Vec::new();
    for suffix in [
        format!("{}.rs", module.name),
        format!("{}/mod.rs", module.name),
    ] {
        let rel = normalize_rel(&join_rel(&base, &suffix));
        if let Some(id) = accept(rel) {
            hits.push(id);
        }
    }
    hits.sort_unstable();
    hits.dedup();
    match hits.as_slice() {
        [] => Targets::None,
        [id] => Targets::One(*id),
        _ => Targets::Blocked,
    }
}

fn roots(
    entries: &[(Option<i64>, String, String)],
    rust_files: &HashSet<i64>,
) -> (
    Vec<i64>,
    Vec<i64>,
    HashSet<i64>,
    HashMap<i64, HashSet<String>>,
) {
    let mut library = Vec::new();
    let mut program = Vec::new();
    let mut program_entries = HashSet::new();
    let mut test_names: HashMap<i64, HashSet<String>> = HashMap::new();
    for (target, kind, reason) in entries {
        let Some(target) = target.filter(|id| rust_files.contains(id)) else {
            continue;
        };
        match kind.as_str() {
            "library_root" => library.push(target),
            "program" => {
                program.push(target);
                program_entries.insert(target);
            }
            "test" => {
                if let Some(name) = reason
                    .strip_prefix("#[test] ")
                    .filter(|name| !name.is_empty())
                {
                    test_names
                        .entry(target)
                        .or_default()
                        .insert(ident(name).to_string());
                }
            }
            _ => {}
        }
    }
    library.sort_unstable();
    library.dedup();
    program.sort_unstable();
    program.dedup();
    (library, program, program_entries, test_names)
}

fn reachable(roots: &[i64], children: &HashMap<i64, HashMap<String, i64>>) -> HashSet<i64> {
    let mut seen = HashSet::new();
    let mut queue: VecDeque<i64> = roots.iter().copied().collect();
    while let Some(file) = queue.pop_front() {
        if !seen.insert(file) {
            continue;
        }
        if let Some(kids) = children.get(&file) {
            queue.extend(kids.values().copied());
        }
    }
    seen
}

/// A file that is a root uses itself. Another root's walk stops at that file, so `crate::`
/// does not jump from a binary root into a library root.
fn assign_crates(
    roots: &HashSet<i64>,
    children: &HashMap<i64, HashMap<String, i64>>,
) -> HashMap<i64, i64> {
    let mut reachers: HashMap<i64, Vec<i64>> = HashMap::new();
    for &root in roots {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([root]);
        while let Some(file) = queue.pop_front() {
            if !seen.insert(file) {
                continue;
            }
            reachers.entry(file).or_default().push(root);
            if file != root && roots.contains(&file) {
                continue;
            }
            if let Some(kids) = children.get(&file) {
                queue.extend(kids.values().copied());
            }
        }
    }
    let mut crate_of: HashMap<i64, i64> = roots.iter().map(|root| (*root, *root)).collect();
    for (file, reached_by) in reachers {
        if roots.contains(&file) {
            continue;
        }
        if reached_by.len() == 1 {
            crate_of.insert(file, reached_by[0]);
        }
    }
    crate_of
}

fn inline_modules(
    symbols: &[Symbol],
    modules: &[ModRef],
    rust_files: &HashSet<i64>,
) -> (HashMap<i64, HashSet<String>>, bool) {
    let mut declared: HashMap<i64, HashSet<String>> = HashMap::new();
    for module in modules {
        if rust_files.contains(&module.file_id) {
            declared
                .entry(module.file_id)
                .or_default()
                .insert(module.name.clone());
        }
    }
    let mut inline_names: HashMap<i64, HashSet<String>> = HashMap::new();
    for symbol in symbols {
        if symbol.kind != "mod" || !rust_files.contains(&symbol.file_id) {
            continue;
        }
        let name = ident(&symbol.name);
        let declared_here = declared
            .get(&symbol.file_id)
            .is_some_and(|names| names.contains(name));
        if !declared_here {
            inline_names
                .entry(symbol.file_id)
                .or_default()
                .insert(name.to_string());
        }
    }
    let has_inline = inline_names.values().any(|names| !names.is_empty());
    (inline_names, has_inline)
}

fn cargo_directories(files: &HashMap<i64, String>) -> Vec<String> {
    let mut dirs: Vec<String> = files
        .values()
        .filter(|path| path.rsplit('/').next().unwrap_or(path) == "Cargo.toml")
        .map(|path| parent_dir(path))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// `None` is the single group of files that sit under no `Cargo.toml`.
fn package_of(rel: &str, cargo_dirs: &[String]) -> Option<String> {
    let dir = parent_dir(rel);
    let mut best: Option<&str> = None;
    for package in cargo_dirs {
        let inside =
            package.is_empty() || dir == *package || dir.starts_with(&format!("{package}/"));
        if inside
            && best
                .map(|current| package.len() > current.len())
                .unwrap_or(true)
        {
            best = Some(package);
        }
    }
    best.map(str::to_string)
}

/// `lib.rs`, `main.rs`, and `mod.rs` keep children beside the file. Every other module keeps
/// them in a directory named after the file stem.
fn module_base(rel_path: &str) -> String {
    let stem = file_stem(rel_path);
    let parent = parent_dir(rel_path);
    if matches!(stem, "lib" | "main" | "mod") {
        parent
    } else {
        join_rel(&parent, stem)
    }
}

pub(super) enum Walk {
    File(i64),
    External,
    Failed,
}

pub(super) fn walk_modules(tree: &Tree, from: i64, segments: &[&str]) -> Walk {
    if segments.is_empty() {
        return Walk::Failed;
    }
    let mut seen = HashSet::new();
    let mut module = match segments[0] {
        "crate" => match tree.crate_root(from) {
            Some(root) => root,
            None => return Walk::Failed,
        },
        "self" => from,
        "super" => match tree.super_module(from) {
            Some(parent) => parent,
            None => return Walk::Failed,
        },
        name => match tree.child(from, name) {
            Some(child) => child,
            None => return Walk::External,
        },
    };
    seen.insert(module);
    for segment in &segments[1..] {
        let next = match *segment {
            "self" => module,
            "super" => match tree.super_module(module) {
                Some(parent) => parent,
                None => return Walk::Failed,
            },
            "crate" => return Walk::Failed,
            name => match tree.child(module, name) {
                Some(child) => child,
                None => return Walk::Failed,
            },
        };
        if !seen.insert(next) {
            return Walk::Failed;
        }
        module = next;
    }
    Walk::File(module)
}

pub(super) fn path_segments(path: &str) -> Option<Vec<&str>> {
    let path = path.trim();
    if path.is_empty() || path.starts_with("::") {
        return None;
    }
    let segments: Vec<&str> = path
        .split("::")
        .filter(|segment| !segment.is_empty())
        .collect();
    (!segments.is_empty()).then_some(segments)
}

fn load_files(conn: &Connection, source_id: i64) -> Result<Vec<(i64, String)>> {
    let mut statement =
        conn.prepare("SELECT id, rel_path FROM knowledge_files WHERE source_id = ?1")?;
    let rows = statement.query_map(params![source_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_symbols(conn: &Connection, source_id: i64) -> Result<Vec<Symbol>> {
    let mut statement = conn.prepare(
        "SELECT id, file_id, name, kind, container, visibility
         FROM knowledge_symbols WHERE source_id = ?1",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(Symbol {
            id: row.get(0)?,
            file_id: row.get(1)?,
            name: row.get(2)?,
            kind: row.get(3)?,
            container: row.get(4)?,
            visibility: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_modules(conn: &Connection, source_id: i64) -> Result<Vec<ModRef>> {
    let mut statement = conn.prepare(
        "SELECT kr.file_id, kf.rel_path, kr.specifier, kr.qualifier
         FROM knowledge_references kr
         JOIN knowledge_files kf ON kf.id = kr.file_id
         WHERE kr.source_id = ?1 AND kr.kind = 'module'",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(ModRef {
            file_id: row.get(0)?,
            from_rel: row.get(1)?,
            name: row.get(2)?,
            qualifier: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_entries(conn: &Connection, source_id: i64) -> Result<Vec<(Option<i64>, String, String)>> {
    let mut statement = conn.prepare(
        "SELECT target_file_id, kind, reason FROM knowledge_entries WHERE source_id = ?1",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

fn extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

fn join_rel(parent: &str, child: &str) -> String {
    let combined = if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    };
    normalize_rel(&combined)
}

fn normalize_rel(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
