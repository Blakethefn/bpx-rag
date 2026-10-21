# Rust retrieval diagnostic labels

This dataset contains 25 existing Rust queries and 611 file-level qrels for the `src/` corpus. It is a diagnostic, model-assisted assessment set, not a human-reviewed benchmark and not suitable for production quality claims.

Four blinded reviewer-agent sessions graded candidates from the hybrid, lexical, and vector pool using the included 0–3 rubric. The pool contained no retrieval ranks or channel labels. Twenty-four candidates received a second independent grade: 17 matched exactly and 7 disagreements received a blind adjudication pass. This limited overlap is not enough to establish reliable ground truth.

The current snapshot contains 55 files and 629 chunks. Its corpus manifest is `6a23c1f59ea38dac296594cb3f49293265702f6fd97d9f963f3bac8b1f52a140`. The evaluation report is `../results/bpx-rag-rust-ai-assisted-v4.json` and records the query, qrel, and corpus hashes.

Re-run against the same source snapshot with:

```bash
bpx-rag evaluate retrieval --dataset eval/bpx-rag-rust-ai-assessed-v1 --corpus-root src --repetitions 1
```

Use independent human review before treating these labels as relevance ground truth. The blind candidate pool and reveal key contain local source excerpts and were kept outside the repository.
