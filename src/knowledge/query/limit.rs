pub(super) const MAX_CANDIDATE_CHUNKS: u32 = 1_000;

pub const MAX_PER_FILE: u32 = 50;

pub const CANDIDATE_OVERSAMPLE_FACTOR: u32 = 5;

pub(crate) fn expanded_candidate_limit(limit: u32) -> u32 {
    limit
        .saturating_mul(CANDIDATE_OVERSAMPLE_FACTOR)
        .min(MAX_CANDIDATE_CHUNKS)
        .max(limit)
}
