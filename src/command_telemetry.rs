use coralogix_cli::telemetry::{self, QueryLanguage};

use super::{Commands, DataprimeCmd, MetricsCmd};

pub(super) fn record_command(command: &Commands, risky_write: bool) {
    let query_language = match command {
        Commands::Logs { .. }
        | Commands::Spans { .. }
        | Commands::Dataprime {
            cmd: DataprimeCmd::Query { .. },
        } => Some(QueryLanguage::Dataprime),
        Commands::Metrics {
            cmd: MetricsCmd::Query { .. } | MetricsCmd::QueryRange { .. },
        } => Some(QueryLanguage::Promql),
        _ => None,
    };
    telemetry::record_command(risky_write, query_language);
}
