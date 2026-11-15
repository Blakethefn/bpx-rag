use super::*;

#[test]
fn go_symbols_cover_type_specs_functions_and_methods() {
    let content = "\
package store

type Entry struct {
	Key string
}

type Reader interface {
	Read() string
}

type Alias = string

func Open(path string) (*Entry, error) {
	return nil, nil
}

func (e *Entry) Close() error {
	return nil
}
";
    let symbols = symbols_for_path(Path::new("store/store.go"), content);
    assert_eq!(
        describe(&symbols),
        vec![
            "struct Entry 3-5",
            "interface Reader 7-9",
            "type Alias 11-11",
            "function Open 13-15",
            "method Close 17-19",
        ]
    );
}
