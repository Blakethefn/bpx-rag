use crate::relations::languages::Grammar;

use super::{listed, references};

#[test]
fn web_grammars_keep_local_include_shapes() {
    assert_eq!(
        references(
            Grammar::Html,
            "<script src=\"app.js\"></script>\n<link href=\"app.css\">\n<a href=\"https://example.com\">x</a>\n<a href=\"#top\">x</a>\n",
        ),
        listed(&[("include", "app.css"), ("include", "app.js")]),
    );
    assert_eq!(
        references(
            Grammar::Css,
            "@import \"theme.css\";\n@import url(\"extra.css\");\n@import url(https://cdn.example/a.css);\n",
        ),
        listed(&[("include", "extra.css"), ("include", "theme.css")]),
    );
}
