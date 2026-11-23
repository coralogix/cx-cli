use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cli_otel::{encode_command_failure_logs, header, PendingBatch, Spool};
use reqwest::{redirect::Policy, Client, StatusCode};
use tokio::time::{sleep, timeout};

use super::delivery::{explicit_headers, retryable, Destination};
use super::key::{self, FetchFailure, Target};

const SENDER_ARG: &str = "__telemetry-send";
const LIFETIME: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const COOLDOWN: Duration = Duration::from_secs(60);
const RETRY_FILE: &str = "retry-after";
const RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(250), Duration::from_secs(1)];

pub(super) fn requested() -> bool {
    env::args_os().nth(1).is_some_and(|arg| arg == SENDER_ARG)
}

pub(super) fn spawn(directory: &Path) {
    if deferred(directory) {
        return;
    }
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let mut command = Command::new(exe);
    command
        .arg(SENDER_ARG)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let _ = command.spawn();
}

pub(super) fn run(build_command: impl FnOnce() -> clap::Command) {
    if !super::consent_allows() {
        return;
    }
    let Some(destination) = Destination::resolve().filter(Destination::is_background) else {
        return;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(send_queued(&destination, build_command));
    // A system DNS lookup can outlive its request timeout.
    runtime.shutdown_background();
}

async fn send_queued(destination: &Destination, build_command: impl FnOnce() -> clap::Command) {
    let Some(directory) = destination.spool_path() else {
        return;
    };
    if deferred(&directory) {
        return;
    }
    let spool = Spool::new(directory.clone());
    let Ok(_lock) = spool.try_lock() else {
        return;
    };
    if deferred(&directory) {
        return;
    }
    if !matches!(
        timeout(
            LIFETIME,
            deliver(destination, &spool, &directory, build_command)
        )
        .await,
        Ok(true)
    ) && super::consent_allows()
    {
        if let Some(now) = now() {
            let retry_at = now.saturating_add(COOLDOWN.as_secs());
            let _ = crate::atomic_file::write(
                &directory.join(RETRY_FILE),
                retry_at.to_string().as_bytes(),
            );
        }
    }
}

async fn pending(spool: &Spool) -> io::Result<Vec<PendingBatch>> {
    for _ in 0..3 {
        match spool.pending() {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                sleep(Duration::from_millis(25)).await;
            }
            result => return result,
        }
    }
    spool.pending()
}

async fn deliver(
    destination: &Destination,
    spool: &Spool,
    directory: &Path,
    build_command: impl FnOnce() -> clap::Command,
) -> bool {
    let Ok(mut batches) = pending(spool).await else {
        return true;
    };
    if batches.is_empty() || !super::consent_allows() {
        return true;
    }

    let _ = rustls::crypto::ring::default_provider().install_default();
    let Ok(client) = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
    else {
        return false;
    };
    let cache = directory.join("telemetry-key.json");
    let Some(mut target) = resolve_target(&client, destination, &cache).await else {
        return false;
    };

    let schema = build_command();
    let mut refreshed = false;
    while !batches.is_empty() {
        for batch in batches {
            if !super::consent_allows() {
                return true;
            }
            let Some(policy) = super::privacy::validate(&batch.payload, &schema) else {
                if spool.remove(&batch.id).is_err() {
                    return true;
                }
                continue;
            };
            let outcome = send_batch(
                &client,
                destination,
                &cache,
                &mut target,
                &mut refreshed,
                &batch.payload,
                policy.allow_failure_logs,
            )
            .await;
            match outcome {
                BatchOutcome::Later => return false,
                BatchOutcome::Delivered | BatchOutcome::Rejected => {
                    if spool.remove(&batch.id).is_err() {
                        return true;
                    }
                }
            }
        }
        if !super::consent_allows() {
            return true;
        }
        let Ok(current) = pending(spool).await else {
            return true;
        };
        batches = current;
    }
    true
}

fn now() -> Option<u64> {
    Some(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs())
}

fn deferred(directory: &Path) -> bool {
    let path = directory.join(RETRY_FILE);
    if !path.is_file() {
        return false;
    }
    let mut value = String::new();
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    if file.take(32).read_to_string(&mut value).is_err() {
        return false;
    }
    let (Ok(expiry), Some(now)) = (value.parse::<u64>(), now()) else {
        return false;
    };
    let delay = expiry.saturating_sub(now);
    (1..=COOLDOWN.as_secs()).contains(&delay)
}

async fn resolve_target(
    client: &Client,
    destination: &Destination,
    cache: &Path,
) -> Option<Target> {
    match destination {
        Destination::Explicit { endpoint, .. } => {
            Some(Target::new(endpoint.clone(), explicit_headers()?))
        }
        Destination::Discovery { key_url } => {
            if let Some(target) = key::load(cache, key_url) {
                return Some(target);
            }
            let mut delays = RETRY_DELAYS.into_iter();
            loop {
                match key::fetch(client, key_url, cache).await {
                    Ok(target) => return Some(target),
                    Err(FetchFailure::Unavailable) => return None,
                    Err(FetchFailure::Temporary) => sleep(delays.next()?).await,
                }
            }
        }
    }
}

enum BatchOutcome {
    Delivered,
    Rejected,
    Later,
}

async fn send_batch(
    client: &Client,
    destination: &Destination,
    cache: &Path,
    target: &mut Target,
    refreshed: &mut bool,
    payload: &[u8],
    allow_failure_logs: bool,
) -> BatchOutcome {
    let outcome = send_payload(
        client,
        destination,
        cache,
        target,
        refreshed,
        Signal::Traces,
        payload,
    )
    .await;
    if matches!(outcome, BatchOutcome::Delivered)
        && allow_failure_logs
        && destination.failure_logs_enabled()
        && target.logs_endpoint.is_some()
    {
        if let Some(logs) = encode_command_failure_logs(payload) {
            return send_payload(
                client,
                destination,
                cache,
                target,
                refreshed,
                Signal::Logs,
                &logs,
            )
            .await;
        }
    }
    outcome
}

#[derive(Clone, Copy)]
enum Signal {
    Traces,
    Logs,
}

async fn send_payload(
    client: &Client,
    destination: &Destination,
    cache: &Path,
    target: &mut Target,
    refreshed: &mut bool,
    signal: Signal,
    payload: &[u8],
) -> BatchOutcome {
    let mut delays = RETRY_DELAYS.into_iter();
    while super::consent_allows() {
        let endpoint = match signal {
            Signal::Traces => &target.endpoint,
            Signal::Logs => match &target.logs_endpoint {
                Some(endpoint) => endpoint,
                None => return BatchOutcome::Rejected,
            },
        };
        let status = client
            .post(endpoint.clone())
            .headers(target.headers.clone())
            .header(header::CONTENT_TYPE, "application/x-protobuf")
            .body(payload.to_vec())
            .send()
            .await
            .map(|response| response.status());

        if let (
            Ok(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
            Destination::Discovery { key_url },
        ) = (&status, destination)
        {
            if !super::consent_allows() {
                return BatchOutcome::Later;
            }
            let _ = std::fs::remove_file(cache);
            if !*refreshed {
                *refreshed = true;
                let Ok(new_target) = key::fetch(client, key_url, cache).await else {
                    return BatchOutcome::Later;
                };
                let replay_trace =
                    matches!(signal, Signal::Logs) && new_target.endpoint != target.endpoint;
                *target = new_target;
                if replay_trace {
                    // Keep both signals together if discovery changes the ingress.
                    return BatchOutcome::Later;
                }
                continue;
            }
            return BatchOutcome::Later;
        }
        if let Ok(status) = status {
            if status.is_success() {
                return BatchOutcome::Delivered;
            }
            if !retryable(status) {
                return BatchOutcome::Rejected;
            }
        }
        let Some(delay) = delays.next() else {
            return BatchOutcome::Later;
        };
        sleep(delay).await;
    }
    BatchOutcome::Later
}
