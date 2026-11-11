use anyhow::{anyhow, Result};

use crate::cli::{EvalAction, PoolAction, PoolCreateArgs, PoolImportArgs};
use crate::knowledge::query::SearchWeights;

pub(crate) fn run_evaluation(action: EvalAction) -> Result<()> {
    match action {
        EvalAction::Retrieval {
            repetitions,
            keyword_weight,
            vector_weight,
            dataset,
            corpus_root,
            layer_policy,
        } => {
            let report = crate::eval::run_retrieval_evaluation(
                repetitions,
                dataset.as_deref(),
                corpus_root.as_deref(),
                SearchWeights {
                    keyword_weight,
                    vector_weight,
                },
                layer_policy,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.filter_leaks > 0
                || report.citation_validity < 1.0
                || !report.citation_content_mismatches.is_empty()
            {
                return Err(anyhow!(
                    "retrieval evaluation found {} filter leaks, citation validity {:.3}, and {} snippet/source mismatches",
                    report.filter_leaks,
                    report.citation_validity,
                    report.citation_content_mismatches.len()
                ));
            }
        }
        EvalAction::Scaling { repetitions } => {
            let report = crate::eval::run_scaling_evaluation(repetitions)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        EvalAction::Embeddings {
            corpus_root,
            glob,
            threads,
            batch_sizes,
            repetitions,
            max_chunks,
        } => {
            let report = crate::eval::run_embedding_benchmark(
                &corpus_root,
                glob.as_deref(),
                threads.as_deref().unwrap_or(&[]),
                &batch_sizes,
                repetitions,
                max_chunks,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        EvalAction::Pool(pool) => match pool.resolve()? {
            PoolAction::Create(args) => generate_pool(args)?,
            PoolAction::Import(args) => import_pool(args)?,
        },
        EvalAction::ImportPool(args) => import_pool(args)?,
    }
    Ok(())
}

fn generate_pool(args: PoolCreateArgs) -> Result<()> {
    let report = crate::eval::generate_judgment_pool(
        args.dataset.as_deref(),
        args.corpus_root.as_deref(),
        &args.output,
        &args.key_output,
        args.depth,
        args.seed,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn import_pool(args: PoolImportArgs) -> Result<()> {
    let imported = crate::eval::import_judgment_pool(
        &args.pool,
        &args.key,
        &args.dataset,
        &args.corpus_root,
        &args.output,
    )?;
    println!(
        "Imported {} explicit relevance grades into {}.",
        imported,
        args.output.display()
    );
    Ok(())
}
