use super::PoolRetrieverRanks;
use crate::knowledge::query::SearchWeights;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::eval) enum PoolChannel {
    Hybrid,
    Lexical,
    Vector,
}

struct ChannelDefinition {
    #[cfg(test)]
    name: &'static str,
    weights: SearchWeights,
    rank_field: fn(&mut PoolRetrieverRanks) -> &mut Option<usize>,
}

impl PoolChannel {
    pub(in crate::eval) const ALL: [Self; 3] = [Self::Hybrid, Self::Lexical, Self::Vector];

    fn definition(self) -> ChannelDefinition {
        match self {
            Self::Hybrid => ChannelDefinition {
                #[cfg(test)]
                name: "hybrid",
                weights: SearchWeights {
                    keyword_weight: 1.0,
                    vector_weight: 1.0,
                },
                rank_field: |ranks| &mut ranks.hybrid,
            },
            Self::Lexical => ChannelDefinition {
                #[cfg(test)]
                name: "lexical",
                weights: SearchWeights {
                    keyword_weight: 1.0,
                    vector_weight: 0.0,
                },
                rank_field: |ranks| &mut ranks.lexical,
            },
            Self::Vector => ChannelDefinition {
                #[cfg(test)]
                name: "vector",
                weights: SearchWeights {
                    keyword_weight: 0.0,
                    vector_weight: 1.0,
                },
                rank_field: |ranks| &mut ranks.vector,
            },
        }
    }

    #[cfg(test)]
    pub(in crate::eval) fn name(self) -> &'static str {
        self.definition().name
    }

    pub(in crate::eval) fn weights(self) -> SearchWeights {
        self.definition().weights
    }

    pub(in crate::eval) fn record_rank(self, ranks: &mut PoolRetrieverRanks, rank: usize) {
        let field = (self.definition().rank_field)(ranks);
        let current = *field;
        *field = Some(current.map_or(rank, |existing| existing.min(rank)));
    }
}
