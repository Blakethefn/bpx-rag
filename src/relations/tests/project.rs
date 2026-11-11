use super::*;

#[test]
fn project_markers_infer_roots_for_go_java_dart_and_csharp() {
    let cases = [
        (
            "go",
            "go.mod",
            "module example.com/project\n",
            "src/main.go",
            "package main\n",
        ),
        (
            "java",
            "pom.xml",
            "<project></project>\n",
            "src/Main.java",
            "class Main {}\n",
        ),
        (
            "dart",
            "pubspec.yaml",
            "name: sample_app\n",
            "lib/main.dart",
            "void main() {}\n",
        ),
        (
            "csharp",
            "Sample.csproj",
            "<Project />\n",
            "src/Main.cs",
            "class Main {}\n",
        ),
        (
            "csharp directory props",
            "Directory.Build.props",
            "<Project />\n",
            "src/Main.cs",
            "class Main {}\n",
        ),
        (
            "csharp slnx",
            "Sample.slnx",
            "<Solution />\n",
            "src/Main.cs",
            "class Main {}\n",
        ),
    ];

    for (name, marker, marker_content, source, source_content) in cases {
        let dir = tempdir().unwrap();
        write_file(dir.path(), marker, marker_content);
        write_file(dir.path(), source, source_content);

        let result = analyze_file(&dir.path().join(source), None).unwrap();

        assert_eq!(
            result.root,
            dir.path().canonicalize().unwrap().display().to_string(),
            "{name}"
        );
    }
}
