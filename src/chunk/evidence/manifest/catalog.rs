use super::super::{ENTRY_PROGRAM, ENTRY_TEST};

pub(super) struct Target {
    pub(super) key: &'static str,
    pub(super) directory: &'static str,
    pub(super) kind: &'static str,
    pub(super) auto: &'static str,
}

impl Target {
    pub(super) fn explicit_reason(&self) -> String {
        format!("Cargo.toml {}", self.key)
    }

    pub(super) fn implicit_reason(&self) -> String {
        format!("Cargo.toml implicit {}", self.key)
    }
}

pub(super) const TARGETS: &[Target] = &[
    Target {
        key: "bin",
        directory: "src/bin",
        kind: ENTRY_PROGRAM,
        auto: "autobins",
    },
    Target {
        key: "test",
        directory: "tests",
        kind: ENTRY_TEST,
        auto: "autotests",
    },
    Target {
        key: "example",
        directory: "examples",
        kind: ENTRY_PROGRAM,
        auto: "autoexamples",
    },
    Target {
        key: "bench",
        directory: "benches",
        kind: ENTRY_PROGRAM,
        auto: "autobenches",
    },
];
