//! Command telemetry integration for cx.

use std::future::Future;

use cli_otel::{ClassifyOutcome, CommandSession, Config, Outcome};

pub async fn run<F>(
    build_command: impl FnOnce() -> clap::Command,
    dispatch: impl FnOnce(clap::ArgMatches) -> F,
) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    let config = Config::from_env("cx")
        .with_service_version(env!("CARGO_PKG_VERSION"))
        .with_coralogix_application("cx-cli", "usage");
    let session = CommandSession::start(config);
    let matches = match session.parse(build_command()) {
        Ok(matches) => matches,
        Err(error) => {
            session.finish(Outcome::from_parse_error(&error)).await;
            error.exit();
        }
    };
    let result = session.in_context(dispatch(matches)).await;
    session.finish(result.classify_outcome()).await;
    match result {
        Err(error) if error.is::<ReportedFailure>() => std::process::exit(1),
        other => other,
    }
}

/// A failure whose complete diagnostic has already been printed by cx.
#[derive(Debug, thiserror::Error)]
pub enum ReportedFailure {
    #[error("Profile selection failed")]
    ProfileSelection,
    #[error("Configuration resolution failed")]
    Configuration,
}
