use super::output::ensure_new_distinct_outputs;
use super::JudgmentPoolGenerationReport;
use anyhow::Result;
use std::path::Path;

mod load;
mod score;
mod write;

pub fn generate_judgment_pool(
    dataset: Option<&Path>,
    corpus_root: Option<&Path>,
    pool_output: &Path,
    key_output: &Path,
    depth: usize,
    seed: u64,
) -> Result<JudgmentPoolGenerationReport> {
    if !(1..=100).contains(&depth) {
        anyhow::bail!("pool depth must be between 1 and 100");
    }
    ensure_new_distinct_outputs(pool_output, key_output)?;
    let loaded = load::load_inputs(dataset, corpus_root)?;
    let (pool_candidates, key_candidates) = score::score_candidates(&loaded, depth, seed)?;
    write::write_outputs(
        pool_output,
        key_output,
        &loaded,
        pool_candidates,
        key_candidates,
        depth,
        seed,
    )
}
