use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::future::Future;

use super::{record_command_metadata, CommandMetadata, Outcome, ParentTrace, Telemetry};

/// The parser's original error (including help/version) or a command failure.
#[derive(Debug)]
pub enum ClapError<E> {
    Parse(clap::Error),
    Command(E),
}

impl<E: fmt::Display> fmt::Display for ClapError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Command(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for ClapError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Command(error) => Some(error),
        }
    }
}

/// Parse and dispatch one CLI invocation, then export its completed spans.
///
/// With telemetry enabled, `before` can enrich the active trace, including on
/// help or parse failures. `classify` runs in the same trace after dispatch and
/// translates the command's original result into a tracing outcome.
/// A process entry point can call `clap::Error::exit` on the returned parse
/// error to preserve clap's streams and exit codes.
pub async fn run_clap<I, A, F, T, E>(
    telemetry: Option<Telemetry>,
    parent: ParentTrace<'_>,
    mut command: clap::Command,
    argv: I,
    before: impl FnOnce(),
    dispatch: impl FnOnce(clap::ArgMatches) -> F,
    classify: impl FnOnce(&Result<T, E>) -> Outcome,
) -> Result<T, ClapError<E>>
where
    I: IntoIterator<Item = A>,
    A: Into<OsString> + Clone,
    F: Future<Output = Result<T, E>>,
{
    let enabled = telemetry.is_some();
    let invocation = async {
        if enabled {
            before();
        }
        let matches = command.try_get_matches_from_mut(argv)?;
        if enabled {
            record_command_metadata(|| CommandMetadata::from_clap(&command, &matches));
        }
        Ok(dispatch(matches).await)
    };

    let result = if let Some(telemetry) = telemetry {
        let result = telemetry
            .run(
                CommandMetadata::default(),
                parent,
                invocation,
                |result| match result {
                    Ok(result) => classify(result),
                    Err(error) => {
                        let exit_code = clap::Error::exit_code(error);
                        Outcome {
                            error_type: (exit_code != 0).then_some("argument_parse"),
                            ..Outcome::completed(exit_code)
                        }
                    }
                },
            )
            .await;
        telemetry.shutdown().await;
        result
    } else {
        invocation.await
    };

    result
        .map_err(ClapError::Parse)?
        .map_err(ClapError::Command)
}
