use super::super::*;
use crate::embed::Embed;

struct EvaluationEmbedder;

impl EvaluationEmbedder {
    fn vector(text: &str) -> Vec<f32> {
        let text = text.to_lowercase();
        if text.contains("prevent users getting in")
            || text.contains("authorization")
            || text.contains("authentication")
            || text.contains("permission policy")
        {
            vec![0.0, 1.0, 0.0, 0.0]
        } else if text.contains("client lifetime")
            || text.contains("postgres connection pool")
            || text.contains("pool max lifetime")
        {
            vec![0.0, 0.0, 1.0, 0.0]
        } else if text.contains("queue acknowledgement")
            || text.contains("queue ack")
            || text.contains("durable queue")
        {
            vec![1.0, 0.0, 0.0, 0.0]
        } else {
            vec![0.0, 0.0, 0.0, 1.0]
        }
    }
}

impl Embed for EvaluationEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        Ok(Self::vector(text))
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|text| Self::vector(text)).collect())
    }

    fn dim(&self) -> usize {
        4
    }
}

mod recall;
