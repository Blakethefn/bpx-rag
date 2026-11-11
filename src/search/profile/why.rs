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
    pub fn matched_layers(&self) -> Vec<&'static str> {
        let mut layers = Vec::new();
        if self.bm25 > 0.0 {
            layers.push("bm25");
        }
        if self.trigram > 0.0 {
            layers.push("trigram");
        }
        if self.fuzzy > 0.0 {
            layers.push("fuzzy");
        }
        if self.identifier > 0.0 {
            layers.push("identifier");
        }
        if self.vector > 0.0 {
            layers.push("vector");
        }
        if self.proximity > 0.0 {
            layers.push("proximity");
        }
        if self.knowledge_bias > 0.0 {
            layers.push("knowledge-bias");
        }
        if self.graph_boost > 0.0 {
            layers.push("graph");
        }
        layers
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
        let mut parts: Vec<String> = self
            .layers
            .matched_layers()
            .into_iter()
            .map(str::to_string)
            .collect();

        if self.metadata_match {
            parts.push("metadata".to_string());
        }
        if self.graph_neighbor {
            parts.push("graph-neighbor".to_string());
        }

        if parts.is_empty() {
            "why: ranking-defaults".to_string()
        } else {
            format!("why: {}", parts.join(", "))
        }
    }

    pub fn detailed_line(&self, final_score: f64) -> String {
        let mut parts = Vec::new();
        if self.layers.bm25 > 0.0 {
            parts.push(format!("bm25={:.4}", self.layers.bm25));
        }
        if self.layers.trigram > 0.0 {
            parts.push(format!("trigram={:.4}", self.layers.trigram));
        }
        if self.layers.fuzzy > 0.0 {
            parts.push(format!("fuzzy={:.4}", self.layers.fuzzy));
        }
        if self.layers.identifier > 0.0 {
            parts.push(format!("identifier={:.4}", self.layers.identifier));
        }
        if self.layers.vector > 0.0 {
            parts.push(format!("vector={:.4}", self.layers.vector));
        }
        if self.layers.proximity > 0.0 {
            parts.push(format!("proximity={:.4}", self.layers.proximity));
        }
        if self.layers.knowledge_bias > 0.0 {
            parts.push(format!("knowledge-bias={:.4}", self.layers.knowledge_bias));
        }
        if self.layers.graph_boost > 0.0 {
            parts.push(format!("graph={:.4}", self.layers.graph_boost));
        }
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
