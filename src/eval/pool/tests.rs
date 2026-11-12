use std::collections::BTreeSet;

use super::*;

#[test]
fn pool_channels_own_the_names_weights_and_rank_fields() {
    let expected = [
        (PoolChannel::Hybrid, "hybrid", 1.0, 1.0),
        (PoolChannel::Lexical, "lexical", 1.0, 0.0),
        (PoolChannel::Vector, "vector", 0.0, 1.0),
    ];
    assert_eq!(PoolChannel::ALL.len(), expected.len());

    let mut ranks = PoolRetrieverRanks::default();
    for (channel, name, keyword_weight, vector_weight) in expected {
        assert!(PoolChannel::ALL.contains(&channel));
        assert_eq!(channel.name(), name);
        let weights = channel.weights();
        assert_eq!(weights.keyword_weight, keyword_weight);
        assert_eq!(weights.vector_weight, vector_weight);

        let mut alone = PoolRetrieverRanks::default();
        channel.record_rank(&mut alone, 4);
        channel.record_rank(&mut alone, 2);
        channel.record_rank(&mut alone, 3);
        assert_eq!(rank_of(&alone, channel), Some(2));
        assert_eq!(set_fields(&alone), 1);

        channel.record_rank(&mut ranks, 1);
    }

    assert_eq!(ranks.hybrid, Some(1));
    assert_eq!(ranks.lexical, Some(1));
    assert_eq!(ranks.vector, Some(1));

    let value = serde_json::to_value(&ranks).unwrap();
    let keys = value
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let names = PoolChannel::ALL
        .into_iter()
        .map(PoolChannel::name)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    assert_eq!(names.len(), PoolChannel::ALL.len());
    assert_eq!(names, keys);
}

fn rank_of(ranks: &PoolRetrieverRanks, channel: PoolChannel) -> Option<usize> {
    match channel {
        PoolChannel::Hybrid => ranks.hybrid,
        PoolChannel::Lexical => ranks.lexical,
        PoolChannel::Vector => ranks.vector,
    }
}

fn set_fields(ranks: &PoolRetrieverRanks) -> usize {
    [ranks.hybrid, ranks.lexical, ranks.vector]
        .into_iter()
        .filter(|rank| rank.is_some())
        .count()
}
