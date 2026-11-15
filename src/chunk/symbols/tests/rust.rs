use super::*;

#[test]
fn rust_symbols_cover_types_modules_functions_methods_and_consts() {
    let content = "\
pub struct Store { id: u32 }
pub enum Mode { Fast, Slow }
pub trait Runner {
    fn run(&self);
}
mod helpers {
    pub fn assist() -> u8 { 1 }
}
impl Store {
    pub fn open(id: u32) -> Self { Store { id } }
}
const LIMIT: u32 = 4;
";
    let symbols = symbols_for_path(Path::new("src/lib.rs"), content);
    assert_eq!(
        describe(&symbols),
        vec![
            "struct Store 1-1",
            "enum Mode 2-2",
            "trait Runner 3-5",
            "mod helpers 6-8",
            "function helpers::assist 7-7",
            "function Store::open 10-10",
            "const LIMIT 12-12",
        ]
    );
}

#[test]
fn rust_visibility_distinguishes_private_from_pub_crate() {
    let content = "\
fn hidden() {}
pub(crate) fn shared() {}
pub fn open() {}
pub(super) fn parent_visible() {}
pub(self) fn local() {}
pub(in crate::util) fn scoped() {}
pub(in super::child) fn child_scoped() {}
pub(in crate) fn crate_in() {}
pub(in self) fn self_in() {}
pub(in super) fn super_in() {}
";
    let symbols = symbols_for_path(Path::new("src/lib.rs"), content);
    let visibility = |name: &str| {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .and_then(|symbol| symbol.visibility.clone())
    };
    assert_eq!(visibility("hidden").as_deref(), Some("private"));
    assert_eq!(visibility("shared").as_deref(), Some("pub(crate)"));
    assert_ne!(visibility("hidden"), visibility("shared"));
    assert_eq!(visibility("open").as_deref(), Some("pub"));
    assert_eq!(visibility("parent_visible").as_deref(), Some("pub(super)"));
    assert_eq!(visibility("local").as_deref(), Some("pub(self)"));
    assert_eq!(visibility("scoped").as_deref(), Some("pub(in crate::util)"));
    assert_eq!(
        visibility("child_scoped").as_deref(),
        Some("pub(in super::child)")
    );
    assert_eq!(visibility("crate_in").as_deref(), Some("pub(in crate)"));
    assert_eq!(visibility("self_in").as_deref(), Some("pub(in self)"));
    assert_eq!(visibility("super_in").as_deref(), Some("pub(in super)"));
}
