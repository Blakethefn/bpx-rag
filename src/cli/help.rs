pub(super) const INIT_ABOUT: &str =
    "Write a default config file when none exists, and leave an existing file unchanged";
pub(super) const INIT_LONG: &str = "Write a default config file when none exists, and leave an existing file unchanged.\n\nCreates the config with the built-in defaults. A file that is already there is left as it is, and this command prints its path. `--init` is still accepted and does the same thing, including when another command is written after it: the config is written and that command is not run.";

pub(super) const REMOVE_ABOUT: &str =
    "Delete one source and its indexed rows. Does not delete files on disk";
pub(super) const REMOVE_LONG: &str = "Delete one source and its indexed rows. Does not delete files on disk.\n\nRemoves the registration and every indexed file, chunk, symbol, and embedding for that source. The directory that was added is left untouched. Indexing it again requires `add`.";

pub(super) const SCRUB_ABOUT: &str =
    "Delete indexed rows that match the sensitive-data policy. Does not delete files on disk";
pub(super) const SCRUB_LONG: &str = "Delete indexed rows that match the sensitive-data policy. Does not delete files on disk.\n\nReads stored index rows and deletes the ones the policy matches. It does not walk the source directory and prints counts rather than paths or file contents. Omit `--source` to scan every registered source.";

pub(super) const FINDINGS_ABOUT: &str =
    "Read stored unused-symbol and unreachable-file conclusions for one source";
pub(super) const FINDINGS_LONG: &str = "Read stored unused-symbol and unreachable-file conclusions for one source.\n\nPrints the analysis row first. Finding rows are included only when that analysis is complete. Omit `--source` only when exactly one source is registered.";

pub(super) const WATCH_ENABLE_ABOUT: &str =
    "Record that a source should be kept in sync. Does not start the watcher";
pub(super) const WATCH_ENABLE_LONG: &str = "Record that a source should be kept in sync. Does not start the watcher.\n\nA watcher that is already running picks the change up within a second or two. Start one with `watch run` or `bpx-rag serve`. `watch on` is still accepted.";

pub(super) const WATCH_DISABLE_ABOUT: &str =
    "Record that a source should not be kept in sync. Does not stop the watcher";
pub(super) const WATCH_DISABLE_LONG: &str = "Record that a source should not be kept in sync. Does not stop the watcher.\n\nA watcher that is already running picks the change up within a second or two and then leaves this source alone. `watch off` is still accepted.";
