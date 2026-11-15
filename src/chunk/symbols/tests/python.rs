use super::*;

#[test]
fn python_symbols_resolve_decorators_and_methods() {
    let content = "\
import os

class Cache:
    def get(self, key):
        return key

@decorator
def load(path):
    return path
";
    let symbols = symbols_for_path(Path::new("app/cache.py"), content);
    assert_eq!(
        describe(&symbols),
        vec![
            "class Cache 3-5",
            "function Cache::get 4-5",
            "function load 8-9",
        ]
    );
}
