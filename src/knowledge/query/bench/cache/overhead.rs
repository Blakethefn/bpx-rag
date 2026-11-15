use std::time::Instant;

use crate::embed::cache::QueryEmbeddingCache;

use super::super::latency::{percentile, unit_vector, MODEL_ID};

fn per_operation_micros(ops: usize, mut batch: impl FnMut(usize)) -> f64 {
    let batch_ms: Vec<f64> = (0..200)
        .map(|round| {
            let start = Instant::now();
            for op in 0..ops {
                batch(round * ops + op);
            }
            start.elapsed().as_secs_f64() * 1_000.0
        })
        .collect();
    percentile(&batch_ms, 0.5) * 1_000.0 / ops as f64
}

#[test]
#[ignore]
fn cache_operation_overhead() {
    let vector = unit_vector(3);
    let full = QueryEmbeddingCache::new(256);
    let keys: Vec<String> = (0..256).map(|index| format!("query {index}")).collect();
    for key in &keys {
        full.insert(MODEL_ID, key, vector.clone());
    }
    let disabled = QueryEmbeddingCache::new(0);

    let hit = per_operation_micros(100, |op| {
        full.get(MODEL_ID, &keys[op % 256]).expect("hit");
    });
    let disabled_get = per_operation_micros(100, |op| {
        assert!(disabled.get(MODEL_ID, &keys[op % 256]).is_none());
    });
    let insert_existing = per_operation_micros(100, |op| {
        full.insert(MODEL_ID, &keys[op % 256], vector.clone());
    });
    let insert_with_eviction = per_operation_micros(100, |op| {
        full.insert(MODEL_ID, &format!("fresh {op}"), vector.clone());
    });
    println!(
        "RESULT overhead hit_get_us={hit:.3} disabled_get_us={disabled_get:.3} \
         insert_existing_us={insert_existing:.3} insert_with_eviction_us={insert_with_eviction:.3}"
    );
}
