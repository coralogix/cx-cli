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

    /// Manage alert scheduler (suppression) rules.
    #[command(after_help = "\
Examples:
  cx alert-schedulers list
  cx alert-schedulers get <rule-id>
  cx alert-schedulers create --from-file rule.json
  cx alert-schedulers delete <rule-id>")]
    AlertSchedulers {
        #[command(subcommand)]
        cmd: AlertSchedulersCmd,
    },

    /// Manage and triage incidents.
    #[command(after_help = "\
Examples:
  cx incidents list
  cx incidents list --severity CRITICAL
  cx incidents get <incident-id>
  cx incidents acknowledge <id1> <id2>
  cx incidents resolve <id>")]
    Incidents {
        #[command(subcommand)]
        cmd: IncidentsCmd,
    },

    /// Manage Events2Metrics definitions.
    #[command(after_help = "\
Examples:
  cx e2m list
  cx e2m get <e2m-id>
  cx e2m create --from-file e2m.json
  cx e2m update --from-file e2m.json
  cx e2m delete <e2m-id>
  cx e2m labels-cardinality
  cx e2m limits")]
    E2m {
        #[command(subcommand)]
        cmd: E2mCmd,
    },

    /// Manage Prometheus recording rule groups.
    #[command(after_help = "\
Examples:
  cx recording-rules list
  cx recording-rules get <group-id>
  cx recording-rules create --from-file rules.json
  cx recording-rules update --from-file rules.json <group-id>
  cx recording-rules delete <group-id>")]
    RecordingRules {
        #[command(subcommand)]
        cmd: RecordingRulesCmd,
    },

    /// Manage SLO definitions.
    #[command(after_help = "\
Examples:
  cx slos list
  cx slos get <slo-id>
  cx slos create --from-file slo.json
  cx slos update --from-file slo.json
  cx slos delete <slo-id>")]
    Slos {
        #[command(subcommand)]
        cmd: SlosCmd,
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
    /// Create a new dashboard from a JSON definition file.
    #[command(after_help = "\
Examples:
  cx dashboards create --from-file dashboard.json
  cx dashboards create --from-file dashboard.json --folder <folder-id>
  cat dashboard.json | cx dashboards create")]
    Create {
        /// Path to a JSON file with the dashboard definition. Use '-' for stdin.
        /// Accepts either a bare dashboard document or a `{\"dashboard\": {...}}` wrapper;
        /// the `requestId` envelope field is generated automatically.
        #[arg(long, default_value = "-")]
        from_file: String,

        /// Optional folder ID to place the dashboard in. Look up with
        /// `cx dashboards folders list`.
        #[arg(long)]
        folder: Option<String>,
    },
    /// Manage dashboard folders.
    Folders {
        #[command(subcommand)]
        cmd: FoldersCmd,
    },
}

#[derive(Subcommand)]
enum FoldersCmd {
    /// List all dashboard folders.
    #[command(after_help = "\
Examples:
  cx dashboards folders list
  cx dashboards folders list -o json")]
    List,
    /// Create a new dashboard folder.
    #[command(after_help = "\
Examples:
  cx dashboards folders create --name \"My Service\"
  cx dashboards folders create --name \"Sub-folder\" --parent-id <folder-id>")]
    Create {
        /// Folder name (required, must be unique within its parent).
        #[arg(long)]
        name: String,

        /// Optional parent folder ID. Omit to create a top-level folder.
        #[arg(long)]
        parent_id: Option<String>,
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
    /// List alert events (trigger instances).
    #[command(after_help = "\
Examples:
  cx alerts events
  cx alerts events --alert-id <id>
  cx alerts events --start now-24h")]
    Events {
        /// Filter by alert definition ID.
        #[arg(long)]
        alert_id: Option<String>,

        /// Start time filter (ISO 8601 or relative).
        #[arg(long)]
        start: Option<String>,

        /// End time filter (ISO 8601 or relative).
        #[arg(long)]
        end: Option<String>,
    },
    /// Get alert event statistics.
    EventStats,
}

#[derive(Subcommand)]
enum IncidentsCmd {
    /// List incidents with optional filters.
    #[command(after_help = "\
Examples:
  cx incidents list
  cx incidents list --severity CRITICAL
  cx incidents list --status TRIGGERED")]
    List {
        /// Filter by status (e.g. TRIGGERED, ACKNOWLEDGED, RESOLVED).
        #[arg(long)]
        status: Option<String>,

        /// Filter by severity (e.g. CRITICAL, WARNING, INFO).
        #[arg(long)]
        severity: Option<String>,

        /// Filter by assignee user ID.
        #[arg(long)]
        assignee: Option<String>,
    },
    /// Get a single incident by ID.
    Get {
        /// Incident ID.
        id: String,
    },
    /// Acknowledge one or more incidents.
    Acknowledge {
        /// Incident IDs to acknowledge.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Resolve one or more incidents.
    Resolve {
        /// Incident IDs to resolve.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Close one or more incidents.
    Close {
        /// Incident IDs to close.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Assign one or more incidents to a user.
    Assign {
        /// Incident IDs to assign.
        #[arg(required = true)]
        ids: Vec<String>,

        /// User ID to assign to.
        #[arg(long)]
        user_id: String,
    },
    /// Unassign one or more incidents.
    Unassign {
        /// Incident IDs to unassign.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// List incident events.
    #[command(after_help = "\
Examples:
  cx incidents events
  cx incidents events --incident-id <id>")]
    Events {
        /// Filter events by incident ID.
        #[arg(long)]
        incident_id: Option<String>,
    },
    /// Get incident aggregations.
    Aggregations,
}

#[derive(Subcommand)]
enum AlertSchedulersCmd {
    /// List all alert scheduler rules.
    List,
    /// Get a single alert scheduler rule by ID.
    Get {
        /// Alert scheduler rule ID.
        id: String,
    },
    /// Create an alert scheduler rule from a JSON definition file.
    #[command(after_help = "\
Examples:
  cx alert-schedulers create --from-file rule.json
  cat rule.json | cx alert-schedulers create")]
    Create {
        /// Path to JSON file with the rule definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update an alert scheduler rule from a JSON definition file.
    Update {
        /// Path to JSON file with the updated rule definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an alert scheduler rule.
    Delete {
        /// Alert scheduler rule ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum E2mCmd {
    /// List all E2M definitions.
    List,
    /// Get a single E2M definition by ID.
    Get {
        /// E2M definition ID.
        id: String,
    },
    /// Create an E2M definition from a JSON file.
    Create {
        /// Path to JSON file with the E2M definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace an E2M definition from a JSON file.
    Update {
        /// Path to JSON file with the updated E2M definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an E2M definition.
    Delete {
        /// E2M definition ID.
        id: String,
    },
    /// Get E2M labels cardinality.
    LabelsCardinality,
    /// Get E2M limits.
    Limits,
}

#[derive(Subcommand)]
enum RecordingRulesCmd {
    /// List all recording rule groups.
    List,
    /// Get a single recording rule group by ID.
    Get {
        /// Recording rule group ID.
        id: String,
    },
    /// Create a recording rule group from a JSON definition file.
    Create {
        /// Path to JSON file with the rule group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a recording rule group from a JSON definition file.
    Update {
        /// Path to JSON file with the updated rule group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,

        /// Recording rule group ID.
        id: String,
    },
    /// Delete a recording rule group.
    Delete {
        /// Recording rule group ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum SlosCmd {
    /// List all SLOs.
    List,
    /// Get a single SLO by ID.
    Get {
        /// SLO ID (UUID).
        id: String,
    },
    /// Create an SLO from a JSON definition file.
    #[command(after_help = "\
Examples:
  cx slos create --from-file slo.json
  cat slo.json | cx slos create")]
    Create {
        /// Path to JSON file with the SLO definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace an SLO definition from a JSON file.
    #[command(after_help = "\
Examples:
  cx slos update --from-file slo.json")]
    Update {
        /// Path to JSON file with the updated SLO definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an SLO.
    Delete {
        /// SLO ID (UUID).
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

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
            DashboardsCmd::Create { from_file, folder } => {
                commands::dashboards::run_create(&targets, &from_file, folder.as_deref(), output)
                    .await?;
            }
            DashboardsCmd::Folders { cmd } => match cmd {
                FoldersCmd::List => {
                    commands::dashboards::run_folders_list(&targets, output).await?;
                }
                FoldersCmd::Create { name, parent_id } => {
                    commands::dashboards::run_folders_create(
                        &targets,
                        &name,
                        parent_id.as_deref(),
                        output,
                    )
                    .await?;
                }
            },
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
            AlertsCmd::Events {
                alert_id,
                start,
                end,
            } => {
                commands::alerts::run_events(
                    &targets,
                    alert_id.as_deref(),
                    start.as_deref(),
                    end.as_deref(),
                    output,
                )
                .await?;
            }
            AlertsCmd::EventStats => {
                commands::alerts::run_event_stats(&targets, output).await?;
            }
        },

        Commands::AlertSchedulers { cmd } => match cmd {
            AlertSchedulersCmd::List => {
                commands::alert_schedulers::run_list(&targets, output).await?;
            }
            AlertSchedulersCmd::Get { id } => {
                commands::alert_schedulers::run_get(&targets, &id, output).await?;
            }
            AlertSchedulersCmd::Create { from_file } => {
                commands::alert_schedulers::run_create(&targets, &from_file, output).await?;
            }
            AlertSchedulersCmd::Update { from_file } => {
                commands::alert_schedulers::run_update(&targets, &from_file, output).await?;
            }
            AlertSchedulersCmd::Delete { id } => {
                commands::alert_schedulers::run_delete(&targets, &id).await?;
            }
        },

        Commands::Incidents { cmd } => match cmd {
            IncidentsCmd::List {
                status,
                severity,
                assignee,
            } => {
                commands::incidents::run_list(
                    &targets,
                    status.as_deref(),
                    severity.as_deref(),
                    assignee.as_deref(),
                    output,
                )
                .await?;
            }
            IncidentsCmd::Get { id } => {
                commands::incidents::run_get(&targets, &id, output).await?;
            }
            IncidentsCmd::Acknowledge { ids } => {
                commands::incidents::run_acknowledge(&targets, &ids).await?;
            }
            IncidentsCmd::Resolve { ids } => {
                commands::incidents::run_resolve(&targets, &ids).await?;
            }
            IncidentsCmd::Close { ids } => {
                commands::incidents::run_close(&targets, &ids).await?;
            }
            IncidentsCmd::Assign { ids, user_id } => {
                commands::incidents::run_assign(&targets, &ids, &user_id).await?;
            }
            IncidentsCmd::Unassign { ids } => {
                commands::incidents::run_unassign(&targets, &ids).await?;
            }
            IncidentsCmd::Events { incident_id } => {
                commands::incidents::run_events(&targets, incident_id.as_deref(), output).await?;
            }
            IncidentsCmd::Aggregations => {
                commands::incidents::run_aggregations(&targets, output).await?;
            }
        },

        Commands::E2m { cmd } => match cmd {
            E2mCmd::List => {
                commands::e2m::run_list(&targets, output).await?;
            }
            E2mCmd::Get { id } => {
                commands::e2m::run_get(&targets, &id, output).await?;
            }
            E2mCmd::Create { from_file } => {
                commands::e2m::run_create(&targets, &from_file, output).await?;
            }
            E2mCmd::Update { from_file } => {
                commands::e2m::run_update(&targets, &from_file, output).await?;
            }
            E2mCmd::Delete { id } => {
                commands::e2m::run_delete(&targets, &id).await?;
            }
            E2mCmd::LabelsCardinality => {
                commands::e2m::run_labels_cardinality(&targets, output).await?;
            }
            E2mCmd::Limits => {
                commands::e2m::run_limits(&targets, output).await?;
            }
        },

        Commands::RecordingRules { cmd } => match cmd {
            RecordingRulesCmd::List => {
                commands::recording_rules::run_list(&targets, output).await?;
            }
            RecordingRulesCmd::Get { id } => {
                commands::recording_rules::run_get(&targets, &id, output).await?;
            }
            RecordingRulesCmd::Create { from_file } => {
                commands::recording_rules::run_create(&targets, &from_file, output).await?;
            }
            RecordingRulesCmd::Update { from_file, id } => {
                commands::recording_rules::run_update(&targets, &id, &from_file, output).await?;
            }
            RecordingRulesCmd::Delete { id } => {
                commands::recording_rules::run_delete(&targets, &id).await?;
            }
        },

        Commands::Slos { cmd } => match cmd {
            SlosCmd::List => {
                commands::slos::run_list(&targets, output).await?;
            }
            SlosCmd::Get { id } => {
                commands::slos::run_get(&targets, &id, output).await?;
            }
            SlosCmd::Create { from_file } => {
                commands::slos::run_create(&targets, &from_file, output).await?;
            }
            SlosCmd::Update { from_file } => {
                commands::slos::run_update(&targets, &from_file, output).await?;
            }
            SlosCmd::Delete { id } => {
                commands::slos::run_delete(&targets, &id).await?;
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
