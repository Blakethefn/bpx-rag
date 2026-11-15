use super::*;

#[test]
fn typescript_symbols_resolve_exports_arrow_functions_and_methods() {
    let content = "\
export interface Config {
  debug: boolean;
}

export function build(config: Config): string {
  return \"\";
}

const parse = (text: string) => text.trim();

class Service {
  run(): void {}
}
";
    let symbols = symbols_for_path(Path::new("web/service.ts"), content);
    assert_eq!(
        describe(&symbols),
        vec![
            "interface Config 1-3",
            "function build 5-7",
            "variable parse 9-9",
            "class Service 11-13",
            "method Service::run 12-12",
        ]
    );
}
