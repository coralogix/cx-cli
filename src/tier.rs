use clap::ValueEnum;

/// Storage tier to search for logs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Tier {
    /// Hot storage: fast, recent data (default).
    #[value(name = "frequent")]
    FrequentSearch,
    /// Cold/archive storage: long-term data.
    #[value(name = "archive")]
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
