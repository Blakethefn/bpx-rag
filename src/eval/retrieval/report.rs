use serde::{Deserialize, Serialize};

mod summary;
pub use summary::{CitationIssue, EvalCaseReport, RetrievalEvalReport};

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswerability {
    #[default]
    Answerable,
    UnanswerableInCorpus,
}

macro_rules! metric_record {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident: f64),* $(,)?
        }
    ) => {
        metric_record! {
            @record
            $(#[$meta])*
            $vis struct $name {}
            rates {
                $($(#[$field_meta])* $field_vis $field: f64),*
            }
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($held:tt)*
        }
        rates {
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident: f64),* $(,)?
        }
    ) => {
        metric_record! {
            @record
            $(#[$meta])*
            $vis struct $name { $($held)* }
            rates {
                $($(#[$field_meta])* $field_vis $field: f64),*
            }
        }
    };
    (
        @record
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($held:tt)*
        }
        rates {
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident: f64),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($held)*
            $(
                $(#[$field_meta])*
                $field_vis $field: f64,
            )*
        }

        impl $name {
            pub(in crate::eval) fn metric_values(&self) -> impl Iterator<Item = f64> + '_ {
                [$(self.$field),*].into_iter()
            }

            fn metric_values_mut(&mut self) -> impl Iterator<Item = &mut f64> {
                [$( &mut self.$field ),*].into_iter()
            }

            pub(in crate::eval) fn accumulate(&mut self, sample: &Self) {
                for (total, value) in self.metric_values_mut().zip(sample.metric_values()) {
                    *total += value;
                }
            }

            pub(in crate::eval) fn mean(&mut self, count: usize) {
                let divisor = count.max(1) as f64;
                for value in self.metric_values_mut() {
                    *value /= divisor;
                }
            }
        }
    };
}

metric_record! {
    #[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
    pub struct QualityMetrics {
        pub recall_at_5: f64,
        pub recall_at_10: f64,
        pub precision_at_5: f64,
        pub precision_at_10: f64,
        pub judged_coverage_at_5: f64,
        pub judged_coverage_at_10: f64,
        pub mrr_at_10: f64,
        pub ndcg_at_10: f64,
        pub success_at_5: f64,
    }
}

metric_record! {
    #[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
    pub struct UnanswerableMetrics {
        pub query_count: usize,
    }
    rates {
        pub explicit_negative_share_at_10: f64,
        pub judged_coverage_at_10: f64,
        pub empty_result_rate: f64,
    }
}

#[cfg(test)]
mod tests;
