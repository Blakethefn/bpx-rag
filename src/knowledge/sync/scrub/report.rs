impl super::SensitiveIndexScrubReport {
    pub fn summary(&self) -> String {
        format!(
            "Sensitive index scrub: removed {} files and {} chunks.",
            self.files_removed, self.chunks_removed
        )
    }
}
