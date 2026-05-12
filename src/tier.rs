use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Storage tier to search for logs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub enum Tier {
    /// Hot storage: fast, recent data (default).
    #[value(name = "frequent")]
    #[serde(rename = "frequent")]
    FrequentSearch,
    /// Cold/archive storage: long-term data.
    #[value(name = "archive")]
    #[serde(rename = "archive")]
    Archive,
}

impl Tier {
    /// Returns the API string for this tier.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            Tier::FrequentSearch => "TIER_FREQUENT_SEARCH",
            Tier::Archive => "TIER_ARCHIVE",
        }
    }
}
