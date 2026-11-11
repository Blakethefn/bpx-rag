use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayerScores {
    pub bm25: f64,
    pub trigram: f64,
    pub fuzzy: f64,
    pub identifier: f64,
    pub vector: f64,
    pub proximity: f64,
    pub knowledge_bias: f64,
    pub graph_boost: f64,
}

impl LayerScores {
    fn positive_layers(&self) -> impl Iterator<Item = (&'static str, f64)> {
        [
            ("bm25", self.bm25),
            ("trigram", self.trigram),
            ("fuzzy", self.fuzzy),
            ("identifier", self.identifier),
            ("vector", self.vector),
            ("proximity", self.proximity),
            ("knowledge-bias", self.knowledge_bias),
            ("graph", self.graph_boost),
        ]
        .into_iter()
        .filter(|(_, score)| *score > 0.0)
    }

    pub fn matched_layers(&self) -> Vec<&'static str> {
        self.positive_layers().map(|(label, _)| label).collect()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchWhy {
    pub layers: LayerScores,
    pub metadata_match: bool,
    pub graph_neighbor: bool,
}

impl SearchWhy {
    pub fn compact_line(&self) -> String {
        let mut parts: Vec<&str> = self.layers.matched_layers();

        if self.metadata_match {
            parts.push("metadata");
        }
        if self.graph_neighbor {
            parts.push("graph-neighbor");
        }

        if parts.is_empty() {
            "why: ranking-defaults".to_string()
        } else {
            format!("why: {}", parts.join(", "))
        }
    }

    pub fn detailed_line(&self, final_score: f64) -> String {
        let mut parts: Vec<String> = self
            .layers
            .positive_layers()
            .map(|(label, score)| format!("{label}={score:.4}"))
            .collect();

        if self.metadata_match {
            parts.push("metadata=true".to_string());
        }
        if self.graph_neighbor {
            parts.push("graph-neighbor=true".to_string());
        }
        parts.push(format!("final={final_score:.4}"));
        format!("why: {}", parts.join(", "))
    }
}

#[cfg(test)]
mod tests;
