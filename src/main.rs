use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use config::OutputFormat;

use cx::commands;
use cx::commands::dataprime::DataprimeFilter;
use cx::config;
use cx::execution::build_targets;
use cx::Tier;

/// Dataset choice for `search-fields`.
#[derive(Debug, Clone, ValueEnum)]
pub enum SearchFieldsDataset {
    Logs,
    Spans,
}

/// Coralogix CLI — query observability data from the terminal.
#[derive(Parser)]
#[command(name = "cx", version, about, long_about = None)]
struct Cli {
    /// Profile(s) to use. Repeat to fan out across multiple profiles simultaneously.
    /// Overrides the default profile set in config.
    #[arg(long, short = 'p', global = true, env = "CX_PROFILE")]
    profile: Vec<String>,

    /// Coralogix API key (overrides a single profile; incompatible with multiple --profile).
    #[arg(long, global = true, env = "CX_API_KEY")]
    api_key: Option<String>,

    /// Coralogix region (overrides a single profile; incompatible with multiple --profile).
    #[arg(long, global = true, env = "CX_REGION")]
    region: Option<String>,

    /// Output format: text, json, or agents. Overrides the default set in config.
    #[arg(long, short = 'o', global = true)]
    output: Option<OutputFormat>,

    #[command(subcommand)]
    command: Commands,
}

/// Separate CLI parser for the `profiles` command — no global API flags.
#[derive(Parser)]
#[command(
    name = "cx",
    version,
    about = "Coralogix CLI — query observability data from the terminal."
)]
struct ProfilesCli {
    #[command(subcommand)]
    command: ProfilesTopLevel,
}

#[derive(Subcommand)]
enum ProfilesTopLevel {
    /// Manage profiles (list, add, delete, set-default).
    Profiles {
        #[command(subcommand)]
        cmd: ProfilesCmd,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Manage profiles (list, add, delete, set-default).
    Profiles {
        #[command(subcommand)]
        cmd: ProfilesCmd,
    },

    /// Remove stale cx_results* files (older than 30 minutes) from the temp directory.
    Cleanup,

    /// Query logs using DataPrime syntax.
    #[command(after_help = "\
Examples:
  cx logs 'filter $m.severity == ERROR'
  cx logs 'filter $d.message ~ \"timeout\"' --start now-6h --tier archive
  cx logs 'filter $l.applicationname == \"api\"' --limit 200 -o json")]
    Logs {
        /// DataPrime query string. e.g. 'filter $m.severity == ERROR'
        query: String,

        /// Start time in ISO 8601 or relative format. e.g. "2024-01-01T00:00:00Z" or "now-1h"
        #[arg(long, default_value = "now-1h")]
        start: String,

        /// End time in ISO 8601 or relative format.
        #[arg(long, default_value = "now")]
        end: String,

        /// Maximum number of results.
        #[arg(long, default_value_t = 100)]
        limit: u32,

        /// Storage tier to search. "frequent" (default) for hot data, "archive" for long-term storage.
        #[arg(long, default_value = "frequent")]
        tier: Tier,
    },

    /// Query metrics using PromQL.
    Metrics {
        #[command(subcommand)]
        cmd: MetricsCmd,
    },

    /// Query spans using DataPrime syntax.
    #[command(after_help = "\
Examples:
  cx spans 'filter $d.traceID == \"abc123\"'
  cx spans 'filter $l.serviceName == \"checkout\"' --start now-2h --limit 50
  cx spans 'groupby $l.operationName aggregate avg($m.duration) as avg_latency'
  cx spans 'filter $m.duration > 1000000' --tier archive -o json")]
    Spans {
        /// DataPrime query string. 'source spans' is automatically prepended if not present.
        query: String,

        /// Start time in ISO 8601 or relative format. e.g. "2024-01-01T00:00:00Z" or "now-1h"
        #[arg(long, default_value = "now-1h")]
        start: String,

        /// End time in ISO 8601 or relative format.
        #[arg(long, default_value = "now")]
        end: String,

        /// Maximum number of results.
        #[arg(long, default_value_t = 200)]
        limit: u32,

        /// Storage tier to search. "frequent" (default) for hot data, "archive" for long-term storage.
        #[arg(long, default_value = "frequent")]
        tier: Tier,
    },

    /// Manage and inspect dashboards.
    Dashboards {
        #[command(subcommand)]
        cmd: DashboardsCmd,
    },

    /// Manage alerts.
    Alerts {
        #[command(subcommand)]
        cmd: AlertsCmd,
    },

    /// Search log/span fields semantically by description.
    #[command(after_help = "\
Examples:
  cx search-fields \"http response status code\"
  cx search-fields \"error severity level\" --dataset spans --limit 10")]
    SearchFields {
        /// Natural-language description to search for (e.g. "http response code").
        text: String,

        /// Dataset to search: logs or spans.
        #[arg(long, default_value = "logs")]
        dataset: SearchFieldsDataset,

        /// Maximum number of results to return.
        #[arg(long, default_value_t = 5)]
        limit: u32,
    },

    /// DataPrime language reference and documentation.
    Dataprime {
        #[command(subcommand)]
        cmd: DataprimeCmd,
    },
}

#[derive(Subcommand)]
enum ProfilesCmd {
    /// List all configured profiles.
    List,
    /// Add or reconfigure a profile interactively.
    Add {
        /// Profile name to configure (default: "default").
        name: Option<String>,
    },
    /// Delete a profile and its stored credentials.
    Delete {
        /// Profile name to delete.
        name: String,
        /// Skip confirmation prompt.
        #[arg(long, short = 'f')]
        force: bool,
    },
    /// Set the default profile.
    SetDefault {
        /// Profile name to set as default.
        name: String,
    },
}

#[derive(Subcommand)]
enum DataprimeCmd {
    /// List all available DataPrime commands and functions.
    List {
        /// Filter by type: commands, functions, or all.
        #[arg(long, default_value = "all")]
        filter: DataprimeFilter,

        /// Filter by name pattern (substring match).
        #[arg(long)]
        name: Option<String>,
    },

    /// Show detailed documentation for a command or function.
    Show {
        /// Name of the command or function.
        name: String,
    },

    /// Execute a raw DataPrime query. Either include a `source` command in the
    /// query itself or use `--source` to set the default source.
    #[command(after_help = "\
Examples:
  cx dataprime query 'source logs | filter $m.severity == \"ERROR\"'
  cx dataprime query --source logs 'filter $m.severity == ERROR'
  cx dataprime query --source spans 'filter $m.duration > 1000000' --start now-6h
  cx dataprime query 'source logs | groupby $l.subsystemname aggregate count()' --limit 50")]
    Query {
        /// DataPrime query string. Include a `source` command in the query
        /// or use --source to set the default source.
        query: String,

        /// Default source for the query (e.g. "logs", "spans"). Equivalent to
        /// starting the query with `source <value>`. Ignored if the query
        /// already contains an explicit `source` command.
        #[arg(long, short = 's')]
        source: Option<String>,

        /// Start time in ISO 8601 or relative format. e.g. "2024-01-01T00:00:00Z" or "now-1h"
        #[arg(long, default_value = "now-1h")]
        start: String,

        /// End time in ISO 8601 or relative format.
        #[arg(long, default_value = "now")]
        end: String,

        /// Maximum number of results.
        #[arg(long, default_value_t = 100)]
        limit: u32,

        /// Storage tier to search. "frequent" (default) for hot data, "archive" for long-term storage.
        #[arg(long, default_value = "frequent")]
        tier: Tier,
    },
}

#[derive(Subcommand)]
enum MetricsCmd {
    /// Execute a PromQL instant query.
    #[command(after_help = "\
Examples:
  cx metrics query 'up'
  cx metrics query 'rate(http_requests_total[5m])' --time 2026-03-21T00:00:00Z")]
    Query {
        /// PromQL expression. e.g. 'up' or 'rate(http_requests_total[5m])'
        expr: String,

        /// Evaluation timestamp (Unix timestamp or RFC3339). Defaults to now.
        #[arg(long)]
        time: Option<String>,
    },

    /// Execute a PromQL range query.
    #[command(after_help = "\
Examples:
  cx metrics query-range 'rate(http_requests_total[5m])'
  cx metrics query-range 'sum by (service) (rate(http_requests_total[5m]))' --start now-6h --step 30s")]
    QueryRange {
        /// PromQL expression. e.g. 'rate(http_requests_total[5m])'
        expr: String,

        /// Start time (Unix timestamp or RFC3339).
        #[arg(long, default_value = "now-1h")]
        start: String,

        /// End time (Unix timestamp or RFC3339).
        #[arg(long, default_value = "now")]
        end: String,

        /// Query resolution step. e.g. "1m", "30s"
        #[arg(long, default_value = "1m")]
        step: String,
    },

    /// Search available metric names.
    #[command(after_help = "\
Examples:
  cx metrics search --name 'http_*'
  cx metrics search --description \"request error rate\"")]
    Search {
        /// Filter by metric name using a substring or wildcard pattern (* matches any sequence).
        #[arg(long, conflicts_with = "description")]
        name: Option<String>,

        /// Filter by description (semantic search using embeddings).
        #[arg(long, conflicts_with = "name")]
        description: Option<String>,
    },

    /// Retrieve all label names for a specific metric.
    GetLabels {
        /// Metric name to retrieve labels for. e.g. 'http_requests_total'
        metric: String,
    },
}

#[derive(Subcommand)]
enum DashboardsCmd {
    /// List all dashboards in the catalog.
    #[command(after_help = "\
Examples:
  cx dashboards catalog
  cx dashboards catalog -o json")]
    Catalog,
    /// Get a single dashboard by ID.
    Get {
        /// Dashboard ID.
        dashboard_id: String,
    },
}

#[derive(Subcommand)]
enum AlertsCmd {
    /// List all alerts.
    #[command(after_help = "\
Examples:
  cx alerts list
  cx alerts list --name \"payment\"")]
    List {
        /// Filter by name (case-insensitive substring match).
        #[arg(long)]
        name: Option<String>,
    },
    /// Get a single alert definition by ID.
    Get {
        /// Alert definition ID or alert version ID (UUID). The alert definition ID is tried
        /// first; if not found, the ID is retried as an alert version ID.
        alert_id: String,
    },
    /// Create an alert from a JSON definition file.
    #[command(after_help = "\
Examples:
  cx alerts create --from-file alert.json
  cat alert.json | cx alerts create")]
    Create {
        /// Path to JSON file with the alert definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Enable an alert.
    Enable {
        /// Alert definition ID (UUID).
        alert_id: String,
    },
    /// Disable an alert.
    Disable {
        /// Alert definition ID (UUID).
        alert_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Check if this is a profiles command — use separate parser without global API flags.
    if std::env::args().nth(1).as_deref() == Some("profiles") {
        let profiles_cli = ProfilesCli::parse();
        let ProfilesTopLevel::Profiles { cmd } = profiles_cli.command;
        return match cmd {
            ProfilesCmd::List => commands::profiles::run_list(),
            ProfilesCmd::Add { name } => commands::profiles::run_add(name).await,
            ProfilesCmd::Delete { name, force } => commands::profiles::run_delete(name, force),
            ProfilesCmd::SetDefault { name } => commands::profiles::run_set_default(name),
        };
    }

    let cli = Cli::parse();

    // Profiles command is handled above; this branch is unreachable but needed for exhaustiveness.
    if let Commands::Profiles { cmd: _ } = cli.command {
        unreachable!("profiles command handled above");
    }

    // Cleanup command doesn't need API credentials.
    if let Commands::Cleanup = cli.command {
        return commands::cleanup::run();
    }

    // Dataprime list/show don't need API credentials — handle them early.
    if let Commands::Dataprime { ref cmd } = cli.command {
        if matches!(cmd, DataprimeCmd::List { .. } | DataprimeCmd::Show { .. }) {
            let global_config = config::load_config().unwrap_or_default();
            let output = cli.output.unwrap_or(global_config.default_output_format);
            return match cmd {
                DataprimeCmd::List { filter, name } => {
                    commands::dataprime::run_list(*filter, name.as_deref(), output)
                }
                DataprimeCmd::Show { name } => commands::dataprime::run_help(name, output),
                // Query needs credentials — handled in the main match below.
                DataprimeCmd::Query { .. } => unreachable!(),
            };
        }
        // DataprimeCmd::Query needs credentials — fall through.
    }

    // Reject --api-key / --region when more than one --profile is supplied,
    // because it would be ambiguous which profile the override targets.
    if cli.profile.len() > 1 && (cli.api_key.is_some() || cli.region.is_some()) {
        bail!(
            "Cannot combine multiple --profile values with --api-key or --region overrides.\n\
             Store per-profile credentials with `cx profiles add <name>`."
        );
    }

    // Load global config for defaults (non-fatal — fall back to defaults).
    let global_config = config::load_config().unwrap_or_default();
    let output = cli.output.unwrap_or(global_config.default_output_format);
    let max_direct = global_config.max_dataprime_direct_output_size;
    let temp_dir = global_config.temp_dir.clone();

    // Resolve one or more profiles into execution targets.
    let configs = config::resolve_all(&cli.profile, cli.api_key.as_deref(), cli.region.as_deref())
        .await
        .map_err(|e| {
            eprintln!("Configuration error: {e}");
            eprintln!("Run `cx profiles add` to set up credentials.");
            e
        })?;

    let targets = build_targets(configs)?;

    match cli.command {
        Commands::Profiles { .. } => unreachable!("handled by ProfilesCli above"),
        Commands::Cleanup => unreachable!("handled above"),

        Commands::Dataprime { cmd } => match cmd {
            DataprimeCmd::List { .. } | DataprimeCmd::Show { .. } => {
                unreachable!("handled above")
            }
            DataprimeCmd::Query {
                query,
                source,
                start,
                end,
                limit,
                tier,
            } => {
                commands::dataprime::run_query(
                    &targets,
                    &query,
                    source.as_deref().unwrap_or(""),
                    &start,
                    &end,
                    limit,
                    tier,
                    output,
                    max_direct,
                    &temp_dir,
                    None,
                )
                .await?;
            }
        },

        Commands::Logs {
            query,
            start,
            end,
            limit,
            tier,
        } => {
            commands::logs::run(
                &targets, &query, &start, &end, limit, tier, output, max_direct, &temp_dir,
            )
            .await?;
        }

        Commands::Metrics { cmd } => match cmd {
            MetricsCmd::Query { expr, time } => {
                commands::metrics::run_query(&targets, &expr, time.as_deref(), output).await?;
            }
            MetricsCmd::QueryRange {
                expr,
                start,
                end,
                step,
            } => {
                commands::metrics::run_query_range(&targets, &expr, &start, &end, &step, output)
                    .await?;
            }
            MetricsCmd::Search { name, description } => {
                commands::metrics::run_search(
                    &targets,
                    name.as_deref(),
                    description.as_deref(),
                    output,
                )
                .await?;
            }
            MetricsCmd::GetLabels { metric } => {
                commands::metrics::run_get_labels(&targets, &metric, output).await?;
            }
        },

        Commands::Spans {
            query,
            start,
            end,
            limit,
            tier,
        } => {
            commands::spans::run(
                &targets, &query, &start, &end, limit, tier, output, max_direct, &temp_dir,
            )
            .await?;
        }

        Commands::Dashboards { cmd } => match cmd {
            DashboardsCmd::Catalog => {
                commands::dashboards::run_catalog(&targets, output).await?;
            }
            DashboardsCmd::Get { dashboard_id } => {
                commands::dashboards::run_get(&targets, &dashboard_id, output).await?;
            }
        },

        Commands::Alerts { cmd } => match cmd {
            AlertsCmd::List { name } => {
                commands::alerts::run_list(&targets, name.as_deref(), output).await?;
            }
            AlertsCmd::Get { alert_id } => {
                commands::alerts::run_get(&targets, &alert_id, output).await?;
            }
            AlertsCmd::Create { from_file } => {
                commands::alerts::run_create(&targets, &from_file, output).await?;
            }
            AlertsCmd::Enable { alert_id } => {
                commands::alerts::run_enable(&targets, &alert_id).await?;
            }
            AlertsCmd::Disable { alert_id } => {
                commands::alerts::run_disable(&targets, &alert_id).await?;
            }
        },

        Commands::SearchFields {
            text,
            dataset,
            limit,
        } => {
            let dataset_str = match dataset {
                SearchFieldsDataset::Logs => "logs",
                SearchFieldsDataset::Spans => "spans",
            };
            commands::search_fields::run(&targets, &text, dataset_str, limit, output).await?;
        }
    }

    Ok(())
}
