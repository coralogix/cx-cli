use std::ffi::OsStr;
use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::aot::Shell;
use clap_complete::engine::ArgValueCompleter;
use clap_complete::env::CompleteEnv;
use clap_complete::CompletionCandidate;
use config::OutputFormat;

use coralogix_cli::commands;
use coralogix_cli::commands::dataprime::DataprimeFilter;
use coralogix_cli::config;
use coralogix_cli::execution::build_targets;
use coralogix_cli::Tier;

/// Returns profile names from `~/.cx/profiles/` as completion candidates.
fn complete_profile_names(current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_str().unwrap_or("");
    config::list_profile_names()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.starts_with(prefix))
        .map(CompletionCandidate::new)
        .collect()
}

/// Dataset choice for `search-fields`.
#[derive(Debug, Clone, ValueEnum)]
pub enum SearchFieldsDataset {
    Logs,
    Spans,
}

/// Coralogix CLI - the observability backbone for AI agents and engineering teams.
#[derive(Parser)]
#[command(
    name = "cx",
    version,
    about,
    long_about = None,
    help_template = "{about-with-newline}\nUsage: {usage}{after-help}\n\nOptions:\n{options}",
    after_help = "\
Query:
  logs               Query logs using DataPrime syntax
  spans              Query spans using DataPrime syntax
  metrics            Query metrics using PromQL
  dataprime          DataPrime language reference and raw queries
  search-fields      Search log/span fields semantically

Observe:
  dashboards         Manage dashboards and dashboard folders
  views              Manage saved views and view folders
  slos               Manage SLO definitions

Detect & Respond:
  alerts             Manage alert definitions and schedulers
  incidents          Manage and triage incidents

Notifications:
  notifications      Manage connectors, routers, presets, and notification testing
  webhooks           Manage outgoing webhooks and automation actions

Data Pipeline:
  rules              Manage log parsing rule groups
  enrichments        Manage enrichment rules and custom enrichment tables
  e2m                Manage Events2Metrics definitions
  recording-rules    Manage Prometheus recording rule groups

Cost & Storage:
  usage              View data usage and consumption metrics
  tco                Manage TCO policies and settings
  retentions         Manage data retention settings
  quotas             Manage quota rules
  archive            Manage data archive storage configuration

Integrations:
  integrations       Manage integrations, extensions, and contextual data

Access:
  iam                Manage API keys, roles, scopes, users, groups, SAML, and IP access

Agent:
  schema             Output the full command tree as JSON for agent consumption

Local:
  profiles           Manage profiles (list, add, delete, set-default)
  cleanup            Remove stale temp files"
)]
struct Cli {
    /// Profile(s) to use. Repeat to fan out across multiple profiles simultaneously.
    /// Overrides the default profile set in config.
    #[arg(long, short = 'p', global = true, env = "CX_PROFILE", add = ArgValueCompleter::new(complete_profile_names))]
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

/// Separate CLI parser for the `profiles` command - no global API flags.
#[derive(Parser)]
#[command(
    name = "cx",
    version,
    about = "Coralogix CLI - the observability backbone for AI agents and engineering teams."
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
enum CompletionsCmd {
    /// Print a completion script to stdout.
    ///
    /// Pipe to a file to install manually, or use `cx completions install` to
    /// let cx choose a default path and track it for future refreshes.
    #[command(after_help = "\
Examples:
  cx completions generate zsh > ~/.zfunc/_cx
  cx completions generate bash > ~/.local/share/bash-completion/completions/cx
  cx completions generate fish > ~/.config/fish/completions/cx.fish")]
    Generate {
        /// Shell to generate completions for.
        shell: Shell,
    },
    /// Install a completion script to a standard path and register it for refresh.
    ///
    /// Default install paths:
    ///   zsh:   ~/.zfunc/_cx
    ///   bash:  ~/.local/share/bash-completion/completions/cx
    ///   fish:  ~/.config/fish/completions/cx.fish
    #[command(after_help = "\
Examples:
  cx completions install zsh
  cx completions install bash
  cx completions install zsh --path /usr/local/share/zsh/site-functions/_cx")]
    Install {
        /// Shell to install completions for.
        shell: Shell,
        /// Override the default install path.
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Regenerate all completion scripts previously installed by cx.
    Refresh,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage profiles (list, add, delete, set-default).
    Profiles {
        #[command(subcommand)]
        cmd: ProfilesCmd,
    },

    /// Generate, install, or refresh shell completion scripts.
    Completions {
        #[command(subcommand)]
        cmd: CompletionsCmd,
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

    /// Manage alert definitions and schedulers.
    Alerts {
        #[command(subcommand)]
        cmd: AlertsCmd,
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

    /// Manage connectors, routers, presets, and notification testing.
    #[command(after_help = "\
Examples:
  cx notifications connectors list
  cx notifications routers list
  cx notifications presets list
  cx notifications test connector --from-file test.json")]
    Notifications {
        #[command(subcommand)]
        cmd: NotificationsCmd,
    },

    /// View data usage and consumption metrics.
    #[command(
        name = "usage",
        after_help = "\
Examples:
  cx usage summary
  cx usage daily --type processed-gbs
  cx usage logs-count
  cx usage spans-count"
    )]
    DataUsage {
        #[command(subcommand)]
        cmd: DataUsageCmd,
    },

    /// Manage TCO (Total Cost of Ownership) policies.
    #[command(
        name = "tco",
        after_help = "\
Examples:
  cx tco list
  cx tco get <policy-id>
  cx tco create --from-file policy.json
  cx tco settings"
    )]
    TcoPolicies {
        #[command(subcommand)]
        cmd: TcoPoliciesCmd,
    },

    /// Manage data retention settings.
    #[command(after_help = "\
Examples:
  cx retentions list
  cx retentions status
  cx retentions update --from-file retentions.json")]
    Retentions {
        #[command(subcommand)]
        cmd: RetentionsCmd,
    },

    /// Manage quota rules.
    #[command(
        name = "quotas",
        after_help = "\
Examples:
  cx quotas get
  cx quotas create --from-file rules.json
  cx quotas update --from-file rules.json
  cx quotas delete"
    )]
    QuotaRules {
        #[command(subcommand)]
        cmd: QuotaRulesCmd,
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

    /// Manage log parsing rule groups.
    #[command(
        name = "rules",
        after_help = "\
Examples:
  cx rules list
  cx rules get <group-id>
  cx rules create --from-file group.json
  cx rules update --from-file group.json <group-id>
  cx rules delete <group-id>
  cx rules usage-limits"
    )]
    RuleGroups {
        #[command(subcommand)]
        cmd: RuleGroupsCmd,
    },

    /// Manage enrichment rules and custom enrichment tables.
    #[command(after_help = "\
Examples:
  cx enrichments list
  cx enrichments add --from-file enrichments.json
  cx enrichments limit
  cx enrichments settings
  cx enrichments custom list")]
    Enrichments {
        #[command(subcommand)]
        cmd: EnrichmentsCmd,
    },

    /// Manage integrations, extensions, and contextual data.
    #[command(after_help = "\
Examples:
  cx integrations list
  cx integrations get <id>
  cx integrations extensions list
  cx integrations contextual-data list")]
    Integrations {
        #[command(subcommand)]
        cmd: IntegrationsCmd,
    },

    /// Manage outgoing webhooks and automation actions.
    #[command(after_help = "\
Examples:
  cx webhooks list
  cx webhooks get <id>
  cx webhooks types
  cx webhooks actions list")]
    Webhooks {
        #[command(subcommand)]
        cmd: WebhooksCmd,
    },

    /// Manage saved views and view folders.
    #[command(after_help = "\
Examples:
  cx views list
  cx views get <id>
  cx views folders list
  cx views folders get <id>")]
    Views {
        #[command(subcommand)]
        cmd: ViewsCmd,
    },

    /// Manage API keys, roles, scopes, users, groups, SAML, and IP access.
    #[command(after_help = "\
Examples:
  cx iam api-keys list
  cx iam roles list
  cx iam scopes list
  cx iam users search
  cx iam groups list
  cx iam saml get
  cx iam ip-access get")]
    Iam {
        #[command(subcommand)]
        cmd: IamCmd,
    },

    /// Manage data archive storage configuration.
    #[command(
        name = "archive",
        after_help = "\
Examples:
  cx archive metrics get
  cx archive logs get"
    )]
    DataArchive {
        #[command(subcommand)]
        cmd: DataArchiveCmd,
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

    /// Output the full command tree as JSON for agent consumption.
    Schema,
}

#[derive(Subcommand)]
enum ProfilesCmd {
    /// List all configured profiles.
    List,
    /// Add or reconfigure a profile interactively.
    Add {
        /// Profile name to configure (default: "default").
        #[arg(add = ArgValueCompleter::new(complete_profile_names))]
        name: Option<String>,
    },
    /// Delete a profile and its stored credentials.
    Delete {
        /// Profile name to delete.
        #[arg(add = ArgValueCompleter::new(complete_profile_names))]
        name: String,
        /// Skip confirmation prompt.
        #[arg(long, short = 'f')]
        force: bool,
    },
    /// Set the default profile.
    SetDefault {
        /// Profile name to set as default.
        #[arg(add = ArgValueCompleter::new(complete_profile_names))]
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
    /// Manage alert scheduler (suppression) rules.
    #[command(after_help = "\
Examples:
  cx alerts schedulers list
  cx alerts schedulers get <rule-id>
  cx alerts schedulers create --from-file rule.json
  cx alerts schedulers delete <rule-id>")]
    Schedulers {
        #[command(subcommand)]
        cmd: AlertSchedulersCmd,
    },
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
  cx alerts schedulers create --from-file rule.json
  cat rule.json | cx alerts schedulers create")]
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
enum NotificationsCmd {
    /// Manage notification connectors.
    #[command(after_help = "\
Examples:
  cx notifications connectors list
  cx notifications connectors get <connector-id>
  cx notifications connectors types")]
    Connectors {
        #[command(subcommand)]
        cmd: ConnectorsCmd,
    },
    /// Manage notification routers.
    #[command(after_help = "\
Examples:
  cx notifications routers list
  cx notifications routers get <router-id>")]
    Routers {
        #[command(subcommand)]
        cmd: RoutersCmd,
    },
    /// Manage notification presets.
    #[command(after_help = "\
Examples:
  cx notifications presets list
  cx notifications presets get <preset-id>")]
    Presets {
        #[command(subcommand)]
        cmd: PresetsCmd,
    },
    /// Test notification configurations.
    Test {
        #[command(subcommand)]
        cmd: NotificationTestCmd,
    },
}

#[derive(Subcommand)]
enum ConnectorsCmd {
    /// List all notification connectors.
    List,
    /// Get a single connector by ID.
    Get { id: String },
    /// Create a connector from a JSON definition file.
    Create {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a connector definition from a JSON file.
    Update {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a connector.
    Delete { id: String },
    /// List connector type summaries.
    Types,
    /// List entity types.
    EntityTypes,
    /// List entity subtypes for a given type.
    EntitySubtypes {
        /// Entity type name.
        #[arg(long, rename_all = "kebab-case")]
        r#type: String,
    },
}

#[derive(Subcommand)]
enum RoutersCmd {
    /// List all notification routers.
    List,
    /// Get a single router by ID.
    Get { id: String },
    /// Create a router from a JSON definition file.
    Create {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a router definition from a JSON file.
    Update {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a router.
    Delete { id: String },
    /// Test entity label matcher.
    ValidateMatcher {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
}

#[derive(Subcommand)]
enum PresetsCmd {
    /// List all notification presets.
    List,
    /// Get a single preset by ID.
    Get { id: String },
    /// Create a custom preset from a JSON definition file.
    Create {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a custom preset from a JSON file.
    Update {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a custom preset.
    Delete { id: String },
    /// Set default preset.
    SetDefault { id: String },
}

#[derive(Subcommand)]
enum NotificationTestCmd {
    /// Test connector configuration.
    Connector {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Test destination.
    Destination {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Test preset configuration.
    Preset {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Test routing condition.
    RoutingCondition {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Test template rendering.
    TemplateRender {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
}

#[derive(Subcommand)]
enum DataUsageCmd {
    /// Show data usage overview.
    Summary {
        /// Start time (ISO 8601 or relative, e.g. now-7d). Defaults to 24h ago.
        #[arg(long)]
        start: Option<String>,

        /// End time (ISO 8601 or relative). Defaults to now.
        #[arg(long)]
        end: Option<String>,
    },
    /// Show daily usage breakdown.
    Daily {
        /// Usage type: processed-gbs, units, or evaluation-tokens.
        #[arg(long, default_value = "processed-gbs")]
        r#type: String,

        /// Start time filter (ISO 8601 or relative).
        #[arg(long)]
        start: Option<String>,

        /// End time filter (ISO 8601 or relative).
        #[arg(long)]
        end: Option<String>,
    },
    /// Show logs count.
    LogsCount,
    /// Show spans count.
    SpansCount,
    /// Show export status.
    ExportStatus,
}

#[derive(Subcommand)]
enum TcoPoliciesCmd {
    /// List all TCO policies.
    List,
    /// Get a single TCO policy by ID.
    Get {
        /// TCO policy ID.
        id: String,
    },
    /// Create a TCO policy from a JSON definition file.
    Create {
        /// Path to JSON file with the policy definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a TCO policy from a JSON definition file.
    Update {
        /// Path to JSON file with the updated policy definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a TCO policy.
    Delete {
        /// TCO policy ID.
        id: String,
    },
    /// Reorder TCO policies by priority.
    Reorder {
        /// Path to JSON file with the reorder definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Test TCO policy matching.
    Test {
        /// Path to JSON file with the test definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Show TCO settings.
    Settings,
    /// Replace TCO settings from a JSON file.
    SettingsUpdate {
        /// Path to JSON file with the settings. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
}

#[derive(Subcommand)]
enum RetentionsCmd {
    /// List retention settings.
    List,
    /// Update retention settings from a JSON file.
    Update {
        /// Path to JSON file with retention settings. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Activate retention.
    Activate,
    /// Check retention enabled status.
    Status,
}

#[derive(Subcommand)]
enum QuotaRulesCmd {
    /// Get quota rule set.
    Get,
    /// Create quota rules from a JSON file.
    Create {
        /// Path to JSON file with quota rules. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace quota rules from a JSON file.
    Update {
        /// Path to JSON file with quota rules. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete quota rules.
    Delete,
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
enum RuleGroupsCmd {
    /// List all rule groups.
    List,
    /// Get a single rule group by ID.
    Get {
        /// Rule group ID.
        id: String,
    },
    /// Create a rule group from a JSON definition file.
    Create {
        /// Path to JSON file with the rule group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a rule group from a JSON definition file.
    Update {
        /// Path to JSON file with the updated rule group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// Rule group ID.
        id: String,
    },
    /// Delete a rule group.
    Delete {
        /// Rule group ID.
        id: String,
    },
    /// Bulk delete rule groups by IDs.
    BulkDelete {
        /// Rule group IDs to delete.
        #[arg(long, num_args = 1..)]
        ids: Vec<String>,
    },
    /// Show rule group usage limits.
    UsageLimits,
}

#[derive(Subcommand)]
enum EnrichmentsCmd {
    /// List all enrichment rules.
    List,
    /// Add enrichment rules from a JSON file.
    Add {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Remove enrichment rules from a JSON file.
    Remove {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Overwrite all enrichment rules from a JSON file.
    Overwrite {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Show enrichment limits.
    Limit,
    /// Show enrichment settings.
    Settings,
    /// Manage custom enrichment tables.
    #[command(after_help = "\
Examples:
  cx enrichments custom list
  cx enrichments custom get <id>
  cx enrichments custom create --from-file table.json
  cx enrichments custom delete <id>")]
    Custom {
        #[command(subcommand)]
        cmd: CustomEnrichmentsCmd,
    },
}

#[derive(Subcommand)]
enum CustomEnrichmentsCmd {
    /// List all custom enrichment tables.
    List,
    /// Get a custom enrichment table by ID.
    Get {
        /// Custom enrichment table ID.
        id: String,
    },
    /// Create a custom enrichment table from a JSON file.
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a custom enrichment table from a JSON file.
    Update {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a custom enrichment table.
    Delete {
        /// Custom enrichment table ID.
        id: String,
    },
    /// Search data in a custom enrichment table.
    Search {
        /// Custom enrichment table ID.
        #[arg(long)]
        id: String,
        /// Search query text.
        #[arg(long)]
        query: String,
    },
}

#[derive(Subcommand)]
enum IntegrationsCmd {
    /// List all integrations.
    List,
    /// Get integration details by ID.
    Get {
        /// Integration ID.
        id: String,
    },
    /// Get integration definition by ID.
    Definition {
        /// Integration ID.
        id: String,
    },
    /// Get deployed integration by ID.
    Deployed {
        /// Integration ID.
        id: String,
    },
    /// Create an integration from a JSON file.
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update an integration from a JSON file.
    Update {
        /// Integration ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an integration.
    Delete {
        /// Integration ID.
        id: String,
    },
    /// Test an integration configuration.
    Test {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Get integration template.
    Template,
    /// Manage extensions.
    #[command(after_help = "\
Examples:
  cx integrations extensions list
  cx integrations extensions deployed
  cx integrations extensions get <id>")]
    Extensions {
        #[command(subcommand)]
        cmd: ExtensionsCmd,
    },
    /// Manage contextual data integrations.
    #[command(after_help = "\
Examples:
  cx integrations contextual-data list
  cx integrations contextual-data get <id>")]
    ContextualData {
        #[command(subcommand)]
        cmd: ContextualDataCmd,
    },
}

#[derive(Subcommand)]
enum ExtensionsCmd {
    /// List all available extensions.
    List,
    /// Get extension details by ID.
    Get {
        /// Extension ID.
        id: String,
    },
    /// List deployed extensions.
    Deployed,
    /// Deploy an extension from a JSON file.
    Deploy {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a deployed extension from a JSON file.
    Update {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Undeploy an extension from a JSON file.
    Undeploy {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
}

#[derive(Subcommand)]
enum WebhooksCmd {
    /// List all outgoing webhooks.
    List,
    /// Get webhook details by ID.
    Get {
        /// Webhook ID.
        id: String,
    },
    /// Create a webhook from a JSON file.
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a webhook from a JSON file.
    Update {
        /// Webhook ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a webhook.
    Delete {
        /// Webhook ID.
        id: String,
    },
    /// Test a webhook by ID.
    Test {
        /// Webhook ID.
        id: String,
    },
    /// List available webhook types.
    Types,
    /// Manage actions (automation hooks).
    #[command(after_help = "\
Examples:
  cx webhooks actions list
  cx webhooks actions get <id>
  cx webhooks actions create --from-file action.json
  cx webhooks actions delete <id>")]
    Actions {
        #[command(subcommand)]
        cmd: ActionsCmd,
    },
}

#[derive(Subcommand)]
enum ContextualDataCmd {
    /// List contextual data integrations.
    List,
    /// Get integration details by ID.
    Get {
        /// Integration ID.
        id: String,
    },
    /// Create an integration from a JSON file.
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update an integration from a JSON file.
    Update {
        /// Integration ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an integration.
    Delete {
        /// Integration ID.
        id: String,
    },
    /// Get integration definition by ID.
    Definition {
        /// Integration ID.
        id: String,
    },
    /// Test an integration by ID.
    Test {
        /// Integration ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum ViewsCmd {
    /// List all saved views.
    List,
    /// Get a view by ID.
    Get {
        /// View ID.
        id: String,
    },
    /// Create a view from a JSON file.
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a view from a JSON file.
    Update {
        /// View ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a view.
    Delete {
        /// View ID.
        id: String,
    },
    /// Manage view folders.
    Folders {
        #[command(subcommand)]
        cmd: ViewFoldersCmd,
    },
}

#[derive(Subcommand)]
enum ViewFoldersCmd {
    /// List all view folders.
    List,
    /// Get a folder by ID.
    Get {
        /// Folder ID.
        id: String,
    },
    /// Create a folder from a JSON file.
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a folder from a JSON file.
    Update {
        /// Folder ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a folder.
    Delete {
        /// Folder ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum IamCmd {
    /// Manage API keys.
    #[command(after_help = "\
Examples:
  cx iam api-keys list
  cx iam api-keys get <id>
  cx iam api-keys create --from-file key.json
  cx iam api-keys send-data-keys
  cx iam api-keys admin list")]
    ApiKeys {
        #[command(subcommand)]
        cmd: ApiKeysCmd,
    },
    /// Manage custom and system roles.
    #[command(after_help = "\
Examples:
  cx iam roles list
  cx iam roles get <id>
  cx iam roles create --from-file role.json
  cx iam roles system")]
    Roles {
        #[command(subcommand)]
        cmd: RolesCmd,
    },
    /// Manage team scopes.
    #[command(after_help = "\
Examples:
  cx iam scopes list
  cx iam scopes get <id>
  cx iam scopes create --from-file scope.json")]
    Scopes {
        #[command(subcommand)]
        cmd: ScopesCmd,
    },
    /// Search and manage team users.
    #[command(after_help = "\
Examples:
  cx iam users search
  cx iam users get <user-id>
  cx iam users create --from-file users.json
  cx iam users set-status --user-ids <id> --status ACTIVE")]
    Users {
        #[command(subcommand)]
        cmd: UsersCmd,
    },
    /// Manage team groups.
    #[command(
        name = "groups",
        after_help = "\
Examples:
  cx iam groups list
  cx iam groups get <id>
  cx iam groups get-by-name <name>
  cx iam groups users <group-id>"
    )]
    TeamGroups {
        #[command(subcommand)]
        cmd: TeamGroupsCmd,
    },
    /// Manage SAML configuration.
    #[command(after_help = "\
Examples:
  cx iam saml get
  cx iam saml sp-params
  cx iam saml set-idp --from-file idp.json
  cx iam saml set-active --active")]
    Saml {
        #[command(subcommand)]
        cmd: SamlCmd,
    },
    /// Manage IP access restrictions.
    #[command(after_help = "\
Examples:
  cx iam ip-access get
  cx iam ip-access create --from-file access.json
  cx iam ip-access update --from-file access.json
  cx iam ip-access delete")]
    IpAccess {
        #[command(subcommand)]
        cmd: IpAccessCmd,
    },
}

#[derive(Subcommand)]
enum ApiKeysCmd {
    /// List all API keys.
    List,
    /// Get a single API key by ID.
    Get {
        /// API key ID.
        id: String,
    },
    /// Create an API key from a JSON definition file.
    Create {
        /// Path to JSON file with the API key definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update an API key from a JSON definition file.
    Update {
        /// Path to JSON file with the updated API key definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// API key ID.
        id: String,
    },
    /// Delete an API key.
    Delete {
        /// API key ID.
        id: String,
    },
    /// List send-data API keys.
    SendDataKeys,
    /// Admin operations on team members' API keys.
    Admin {
        #[command(subcommand)]
        cmd: ApiKeysAdminCmd,
    },
}

#[derive(Subcommand)]
enum ApiKeysAdminCmd {
    /// List all team members' API keys.
    List,
    /// Bulk delete API keys by IDs.
    Delete {
        /// API key IDs to delete.
        #[arg(long, num_args = 1..)]
        ids: Vec<String>,
    },
    /// Set active/inactive status for API keys.
    SetStatus {
        /// API key IDs to update.
        #[arg(long, num_args = 1..)]
        ids: Vec<String>,
        /// Whether to activate or deactivate the keys.
        #[arg(long)]
        active: bool,
    },
}

#[derive(Subcommand)]
enum RolesCmd {
    /// List all custom roles.
    List,
    /// Get a single custom role by ID.
    Get {
        /// Role ID.
        id: String,
    },
    /// Create a custom role from a JSON definition file.
    Create {
        /// Path to JSON file with the role definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a custom role from a JSON definition file.
    Update {
        /// Path to JSON file with the updated role definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// Role ID.
        id: String,
    },
    /// Delete a custom role.
    Delete {
        /// Role ID.
        id: String,
    },
    /// List system roles.
    System,
}

#[derive(Subcommand)]
enum ScopesCmd {
    /// List all scopes.
    List,
    /// Get a single scope by ID.
    Get {
        /// Scope ID.
        id: String,
    },
    /// Create a scope from a JSON definition file.
    Create {
        /// Path to JSON file with the scope definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a scope from a JSON definition file.
    Update {
        /// Path to JSON file with the updated scope definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a scope.
    Delete {
        /// Scope ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum UsersCmd {
    /// Search users.
    Search {
        /// Search query string.
        #[arg(long)]
        query: Option<String>,
        /// Filter by status.
        #[arg(long)]
        status: Option<String>,
        /// Maximum number of results per page.
        #[arg(long)]
        page_size: Option<String>,
        /// Pagination token.
        #[arg(long)]
        page_token: Option<String>,
    },
    /// Get a single user by ID.
    Get {
        /// User ID.
        user_id: String,
    },
    /// Create user(s) from a JSON definition file.
    Create {
        /// Path to JSON file with the user definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update user(s) from a JSON definition file.
    Update {
        /// Path to JSON file with the updated user definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Set status for one or more users.
    SetStatus {
        /// User IDs to update.
        #[arg(long, num_args = 1..)]
        user_ids: Vec<String>,
        /// Status to set (e.g. ACTIVE, INACTIVE).
        #[arg(long)]
        status: String,
    },
}

#[derive(Subcommand)]
enum TeamGroupsCmd {
    /// List all team groups.
    List {},
    /// Get a single team group by ID.
    Get {
        /// Team group ID.
        id: String,
    },
    /// Get a team group by name.
    GetByName {
        /// Team group name.
        name: String,
    },
    /// List users in a team group.
    Users {
        /// Team group ID.
        group_id: String,
    },
    /// Create a team group from a JSON definition file.
    Create {
        /// Path to JSON file with the group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a team group from a JSON definition file.
    Update {
        /// Path to JSON file with the updated group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// Team group ID.
        id: String,
    },
    /// Delete a team group.
    Delete {
        /// Team group ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum SamlCmd {
    /// Get SAML configuration.
    Get,
    /// Get SAML service provider parameters.
    SpParams,
    /// Set SAML IDP parameters from a JSON file.
    SetIdp {
        /// Path to JSON file with IDP parameters. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Activate or deactivate SAML.
    SetActive {
        /// Whether to activate SAML.
        #[arg(long)]
        active: bool,
    },
}

#[derive(Subcommand)]
enum IpAccessCmd {
    /// Get IP access settings.
    Get,
    /// Create IP access settings from a JSON file.
    Create {
        /// Path to JSON file with IP access settings. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update IP access settings from a JSON file.
    Update {
        /// Path to JSON file with updated IP access settings. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete IP access settings.
    Delete,
}

#[derive(Subcommand)]
enum ActionsCmd {
    /// List all actions.
    List,
    /// Get action details by ID.
    Get {
        /// Action ID.
        id: String,
    },
    /// Create an action from a JSON file.
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace an action from a JSON file.
    Update {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an action.
    Delete {
        /// Action ID.
        id: String,
    },
    /// Batch execute actions from a JSON file.
    Batch {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Reorder actions from a JSON file.
    Reorder {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
}

#[derive(Subcommand)]
enum DataArchiveCmd {
    /// Manage metrics archive.
    Metrics {
        #[command(subcommand)]
        cmd: DataArchiveMetricsCmd,
    },
    /// Manage logs archive.
    Logs {
        #[command(subcommand)]
        cmd: DataArchiveLogsCmd,
    },
}

#[derive(Subcommand)]
enum DataArchiveMetricsCmd {
    /// Get metrics archive configuration.
    Get,
    /// Create metrics archive from a JSON file.
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update metrics archive from a JSON file.
    Update {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Enable metrics archiving.
    Enable,
    /// Disable metrics archiving.
    Disable,
    /// Validate metrics archive configuration from a JSON file.
    Validate {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
}

#[derive(Subcommand)]
enum DataArchiveLogsCmd {
    /// Get logs archive target.
    Get,
    /// Set logs archive target from a JSON file.
    Set {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
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
    // Handle shell completions before any stdout output.
    // When the COMPLETE env var is set (e.g. `COMPLETE=zsh cx`), this generates
    // completion scripts and exits. Otherwise it is a no-op.
    CompleteEnv::with_factory(Cli::command).complete();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Check if this is a profiles command - use separate parser without global API flags.
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

    // Schema command doesn't need API credentials - outputs command tree as JSON.
    if let Commands::Schema = cli.command {
        return commands::schema::run(Cli::command());
    }

    // Completions commands don't need API credentials.
    if let Commands::Completions { cmd } = cli.command {
        return match cmd {
            CompletionsCmd::Generate { shell } => {
                commands::completions::run_generate(shell, &mut Cli::command())
            }
            CompletionsCmd::Install { shell, path } => {
                commands::completions::run_install(shell, path, &mut Cli::command())
            }
            CompletionsCmd::Refresh => commands::completions::run_refresh(Cli::command),
        };
    }

    // Dataprime list/show don't need API credentials - handle them early.
    if let Commands::Dataprime { ref cmd } = cli.command {
        if matches!(cmd, DataprimeCmd::List { .. } | DataprimeCmd::Show { .. }) {
            let global_config = config::load_config().unwrap_or_default();
            let output = cli.output.unwrap_or(global_config.default_output_format);
            return match cmd {
                DataprimeCmd::List { filter, name } => {
                    commands::dataprime::run_list(*filter, name.as_deref(), output)
                }
                DataprimeCmd::Show { name } => commands::dataprime::run_help(name, output),
                // Query needs credentials - handled in the main match below.
                DataprimeCmd::Query { .. } => unreachable!(),
            };
        }
        // DataprimeCmd::Query needs credentials - fall through.
    }

    // Reject --api-key / --region when more than one --profile is supplied,
    // because it would be ambiguous which profile the override targets.
    if cli.profile.len() > 1 && (cli.api_key.is_some() || cli.region.is_some()) {
        bail!(
            "Cannot combine multiple --profile values with --api-key or --region overrides.\n\
             Store per-profile credentials with `cx profiles add <name>`."
        );
    }

    // Load global config for defaults (non-fatal - fall back to defaults).
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
        Commands::Schema => unreachable!("handled above"),
        Commands::Completions { .. } => unreachable!("handled above"),

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
            AlertsCmd::Schedulers { cmd } => match cmd {
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

        Commands::Notifications { cmd } => match cmd {
            NotificationsCmd::Connectors { cmd } => match cmd {
                ConnectorsCmd::List => {
                    commands::connectors::run_list(&targets, output).await?;
                }
                ConnectorsCmd::Get { id } => {
                    commands::connectors::run_get(&targets, &id, output).await?;
                }
                ConnectorsCmd::Create { from_file } => {
                    commands::connectors::run_create(&targets, &from_file, output).await?;
                }
                ConnectorsCmd::Update { from_file } => {
                    commands::connectors::run_update(&targets, &from_file, output).await?;
                }
                ConnectorsCmd::Delete { id } => {
                    commands::connectors::run_delete(&targets, &id).await?;
                }
                ConnectorsCmd::Types => {
                    commands::connectors::run_types(&targets, output).await?;
                }
                ConnectorsCmd::EntityTypes => {
                    commands::connectors::run_entity_types(&targets, output).await?;
                }
                ConnectorsCmd::EntitySubtypes { r#type } => {
                    commands::connectors::run_entity_subtypes(&targets, &r#type, output).await?;
                }
            },
            NotificationsCmd::Routers { cmd } => match cmd {
                RoutersCmd::List => {
                    commands::routers::run_list(&targets, output).await?;
                }
                RoutersCmd::Get { id } => {
                    commands::routers::run_get(&targets, &id, output).await?;
                }
                RoutersCmd::Create { from_file } => {
                    commands::routers::run_create(&targets, &from_file, output).await?;
                }
                RoutersCmd::Update { from_file } => {
                    commands::routers::run_update(&targets, &from_file, output).await?;
                }
                RoutersCmd::Delete { id } => {
                    commands::routers::run_delete(&targets, &id).await?;
                }
                RoutersCmd::ValidateMatcher { from_file } => {
                    commands::routers::run_validate_matcher(&targets, &from_file, output).await?;
                }
            },
            NotificationsCmd::Presets { cmd } => match cmd {
                PresetsCmd::List => {
                    commands::presets::run_list(&targets, output).await?;
                }
                PresetsCmd::Get { id } => {
                    commands::presets::run_get(&targets, &id, output).await?;
                }
                PresetsCmd::Create { from_file } => {
                    commands::presets::run_create(&targets, &from_file, output).await?;
                }
                PresetsCmd::Update { from_file } => {
                    commands::presets::run_update(&targets, &from_file, output).await?;
                }
                PresetsCmd::Delete { id } => {
                    commands::presets::run_delete(&targets, &id).await?;
                }
                PresetsCmd::SetDefault { id } => {
                    commands::presets::run_set_default(&targets, &id).await?;
                }
            },
            NotificationsCmd::Test { cmd } => match cmd {
                NotificationTestCmd::Connector { from_file } => {
                    commands::notification_testing::run_test_connector(
                        &targets, &from_file, output,
                    )
                    .await?;
                }
                NotificationTestCmd::Destination { from_file } => {
                    commands::notification_testing::run_test_destination(
                        &targets, &from_file, output,
                    )
                    .await?;
                }
                NotificationTestCmd::Preset { from_file } => {
                    commands::notification_testing::run_test_preset(&targets, &from_file, output)
                        .await?;
                }
                NotificationTestCmd::RoutingCondition { from_file } => {
                    commands::notification_testing::run_test_routing_condition(
                        &targets, &from_file, output,
                    )
                    .await?;
                }
                NotificationTestCmd::TemplateRender { from_file } => {
                    commands::notification_testing::run_test_template_render(
                        &targets, &from_file, output,
                    )
                    .await?;
                }
            },
        },

        Commands::DataUsage { cmd } => match cmd {
            DataUsageCmd::Summary { start, end } => {
                commands::data_usage::run_summary(
                    &targets,
                    start.as_deref(),
                    end.as_deref(),
                    output,
                )
                .await?;
            }
            DataUsageCmd::Daily { r#type, start, end } => {
                commands::data_usage::run_daily(
                    &targets,
                    &r#type,
                    start.as_deref(),
                    end.as_deref(),
                    output,
                )
                .await?;
            }
            DataUsageCmd::LogsCount => {
                commands::data_usage::run_logs_count(&targets, output).await?;
            }
            DataUsageCmd::SpansCount => {
                commands::data_usage::run_spans_count(&targets, output).await?;
            }
            DataUsageCmd::ExportStatus => {
                commands::data_usage::run_export_status(&targets, output).await?;
            }
        },

        Commands::TcoPolicies { cmd } => match cmd {
            TcoPoliciesCmd::List => {
                commands::tco_policies::run_list(&targets, output).await?;
            }
            TcoPoliciesCmd::Get { id } => {
                commands::tco_policies::run_get(&targets, &id, output).await?;
            }
            TcoPoliciesCmd::Create { from_file } => {
                commands::tco_policies::run_create(&targets, &from_file, output).await?;
            }
            TcoPoliciesCmd::Update { from_file } => {
                commands::tco_policies::run_update(&targets, &from_file, output).await?;
            }
            TcoPoliciesCmd::Delete { id } => {
                commands::tco_policies::run_delete(&targets, &id).await?;
            }
            TcoPoliciesCmd::Reorder { from_file } => {
                commands::tco_policies::run_reorder(&targets, &from_file, output).await?;
            }
            TcoPoliciesCmd::Test { from_file } => {
                commands::tco_policies::run_test(&targets, &from_file, output).await?;
            }
            TcoPoliciesCmd::Settings => {
                commands::tco_policies::run_settings(&targets, output).await?;
            }
            TcoPoliciesCmd::SettingsUpdate { from_file } => {
                commands::tco_policies::run_settings_update(&targets, &from_file, output).await?;
            }
        },

        Commands::Retentions { cmd } => match cmd {
            RetentionsCmd::List => {
                commands::retentions::run_list(&targets, output).await?;
            }
            RetentionsCmd::Update { from_file } => {
                commands::retentions::run_update(&targets, &from_file, output).await?;
            }
            RetentionsCmd::Activate => {
                commands::retentions::run_activate(&targets).await?;
            }
            RetentionsCmd::Status => {
                commands::retentions::run_status(&targets, output).await?;
            }
        },

        Commands::QuotaRules { cmd } => match cmd {
            QuotaRulesCmd::Get => {
                commands::quota_rules::run_get(&targets, output).await?;
            }
            QuotaRulesCmd::Create { from_file } => {
                commands::quota_rules::run_create(&targets, &from_file, output).await?;
            }
            QuotaRulesCmd::Update { from_file } => {
                commands::quota_rules::run_update(&targets, &from_file, output).await?;
            }
            QuotaRulesCmd::Delete => {
                commands::quota_rules::run_delete(&targets).await?;
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

        Commands::RuleGroups { cmd } => match cmd {
            RuleGroupsCmd::List => {
                commands::rule_groups::run_list(&targets, output).await?;
            }
            RuleGroupsCmd::Get { id } => {
                commands::rule_groups::run_get(&targets, &id, output).await?;
            }
            RuleGroupsCmd::Create { from_file } => {
                commands::rule_groups::run_create(&targets, &from_file, output).await?;
            }
            RuleGroupsCmd::Update { from_file, id } => {
                commands::rule_groups::run_update(&targets, &id, &from_file, output).await?;
            }
            RuleGroupsCmd::Delete { id } => {
                commands::rule_groups::run_delete(&targets, &id).await?;
            }
            RuleGroupsCmd::BulkDelete { ids } => {
                commands::rule_groups::run_bulk_delete(&targets, &ids).await?;
            }
            RuleGroupsCmd::UsageLimits => {
                commands::rule_groups::run_usage_limits(&targets, output).await?;
            }
        },

        Commands::Enrichments { cmd } => match cmd {
            EnrichmentsCmd::List => {
                commands::enrichments::run_list(&targets, output).await?;
            }
            EnrichmentsCmd::Add { from_file } => {
                commands::enrichments::run_add(&targets, &from_file, output).await?;
            }
            EnrichmentsCmd::Remove { from_file } => {
                commands::enrichments::run_remove(&targets, &from_file, output).await?;
            }
            EnrichmentsCmd::Overwrite { from_file } => {
                commands::enrichments::run_overwrite(&targets, &from_file, output).await?;
            }
            EnrichmentsCmd::Limit => {
                commands::enrichments::run_limit(&targets, output).await?;
            }
            EnrichmentsCmd::Settings => {
                commands::enrichments::run_settings(&targets, output).await?;
            }
            EnrichmentsCmd::Custom { cmd } => match cmd {
                CustomEnrichmentsCmd::List => {
                    commands::custom_enrichments::run_list(&targets, output).await?;
                }
                CustomEnrichmentsCmd::Get { id } => {
                    commands::custom_enrichments::run_get(&targets, &id, output).await?;
                }
                CustomEnrichmentsCmd::Create { from_file } => {
                    commands::custom_enrichments::run_create(&targets, &from_file, output).await?;
                }
                CustomEnrichmentsCmd::Update { from_file } => {
                    commands::custom_enrichments::run_update(&targets, &from_file, output).await?;
                }
                CustomEnrichmentsCmd::Delete { id } => {
                    commands::custom_enrichments::run_delete(&targets, &id).await?;
                }
                CustomEnrichmentsCmd::Search { id, query } => {
                    commands::custom_enrichments::run_search(&targets, &id, &query, output).await?;
                }
            },
        },

        Commands::Integrations { cmd } => match cmd {
            IntegrationsCmd::List => {
                commands::integrations::run_list(&targets, output).await?;
            }
            IntegrationsCmd::Get { id } => {
                commands::integrations::run_get(&targets, &id, output).await?;
            }
            IntegrationsCmd::Definition { id } => {
                commands::integrations::run_definition(&targets, &id, output).await?;
            }
            IntegrationsCmd::Deployed { id } => {
                commands::integrations::run_deployed(&targets, &id, output).await?;
            }
            IntegrationsCmd::Create { from_file } => {
                commands::integrations::run_create(&targets, &from_file, output).await?;
            }
            IntegrationsCmd::Update { id, from_file } => {
                commands::integrations::run_update(&targets, &id, &from_file, output).await?;
            }
            IntegrationsCmd::Delete { id } => {
                commands::integrations::run_delete(&targets, &id).await?;
            }
            IntegrationsCmd::Test { from_file } => {
                commands::integrations::run_test(&targets, &from_file, output).await?;
            }
            IntegrationsCmd::Template => {
                commands::integrations::run_template(&targets, output).await?;
            }
            IntegrationsCmd::Extensions { cmd } => match cmd {
                ExtensionsCmd::List => {
                    commands::extensions::run_list(&targets, output).await?;
                }
                ExtensionsCmd::Get { id } => {
                    commands::extensions::run_get(&targets, &id, output).await?;
                }
                ExtensionsCmd::Deployed => {
                    commands::extensions::run_deployed(&targets, output).await?;
                }
                ExtensionsCmd::Deploy { from_file } => {
                    commands::extensions::run_deploy(&targets, &from_file, output).await?;
                }
                ExtensionsCmd::Update { from_file } => {
                    commands::extensions::run_update(&targets, &from_file, output).await?;
                }
                ExtensionsCmd::Undeploy { from_file } => {
                    commands::extensions::run_undeploy(&targets, &from_file, output).await?;
                }
            },
            IntegrationsCmd::ContextualData { cmd } => match cmd {
                ContextualDataCmd::List => {
                    commands::contextual_data::run_list(&targets, output).await?;
                }
                ContextualDataCmd::Get { id } => {
                    commands::contextual_data::run_get(&targets, &id, output).await?;
                }
                ContextualDataCmd::Create { from_file } => {
                    commands::contextual_data::run_create(&targets, &from_file, output).await?;
                }
                ContextualDataCmd::Update { id, from_file } => {
                    commands::contextual_data::run_update(&targets, &id, &from_file, output)
                        .await?;
                }
                ContextualDataCmd::Delete { id } => {
                    commands::contextual_data::run_delete(&targets, &id).await?;
                }
                ContextualDataCmd::Definition { id } => {
                    commands::contextual_data::run_definition(&targets, &id, output).await?;
                }
                ContextualDataCmd::Test { id } => {
                    commands::contextual_data::run_test(&targets, &id, output).await?;
                }
            },
        },

        Commands::Webhooks { cmd } => match cmd {
            WebhooksCmd::List => {
                commands::webhooks::run_list(&targets, output).await?;
            }
            WebhooksCmd::Get { id } => {
                commands::webhooks::run_get(&targets, &id, output).await?;
            }
            WebhooksCmd::Create { from_file } => {
                commands::webhooks::run_create(&targets, &from_file, output).await?;
            }
            WebhooksCmd::Update { id, from_file } => {
                commands::webhooks::run_update(&targets, &id, &from_file, output).await?;
            }
            WebhooksCmd::Delete { id } => {
                commands::webhooks::run_delete(&targets, &id).await?;
            }
            WebhooksCmd::Test { id } => {
                commands::webhooks::run_test(&targets, &id, output).await?;
            }
            WebhooksCmd::Types => {
                commands::webhooks::run_types(&targets, output).await?;
            }
            WebhooksCmd::Actions { cmd } => match cmd {
                ActionsCmd::List => {
                    commands::actions::run_list(&targets, output).await?;
                }
                ActionsCmd::Get { id } => {
                    commands::actions::run_get(&targets, &id, output).await?;
                }
                ActionsCmd::Create { from_file } => {
                    commands::actions::run_create(&targets, &from_file, output).await?;
                }
                ActionsCmd::Update { from_file } => {
                    commands::actions::run_update(&targets, &from_file, output).await?;
                }
                ActionsCmd::Delete { id } => {
                    commands::actions::run_delete(&targets, &id).await?;
                }
                ActionsCmd::Batch { from_file } => {
                    commands::actions::run_batch(&targets, &from_file, output).await?;
                }
                ActionsCmd::Reorder { from_file } => {
                    commands::actions::run_reorder(&targets, &from_file, output).await?;
                }
            },
        },

        Commands::Views { cmd } => match cmd {
            ViewsCmd::List => {
                commands::views::run_list(&targets, output).await?;
            }
            ViewsCmd::Get { id } => {
                commands::views::run_get(&targets, &id, output).await?;
            }
            ViewsCmd::Create { from_file } => {
                commands::views::run_create(&targets, &from_file, output).await?;
            }
            ViewsCmd::Update { id, from_file } => {
                commands::views::run_update(&targets, &id, &from_file, output).await?;
            }
            ViewsCmd::Delete { id } => {
                commands::views::run_delete(&targets, &id).await?;
            }
            ViewsCmd::Folders { cmd } => match cmd {
                ViewFoldersCmd::List => {
                    commands::views::run_folders_list(&targets, output).await?;
                }
                ViewFoldersCmd::Get { id } => {
                    commands::views::run_folders_get(&targets, &id, output).await?;
                }
                ViewFoldersCmd::Create { from_file } => {
                    commands::views::run_folders_create(&targets, &from_file, output).await?;
                }
                ViewFoldersCmd::Update { id, from_file } => {
                    commands::views::run_folders_update(&targets, &id, &from_file, output).await?;
                }
                ViewFoldersCmd::Delete { id } => {
                    commands::views::run_folders_delete(&targets, &id).await?;
                }
            },
        },

        Commands::Iam { cmd } => match cmd {
            IamCmd::ApiKeys { cmd } => match cmd {
                ApiKeysCmd::List => {
                    commands::api_keys::run_list(&targets, output).await?;
                }
                ApiKeysCmd::Get { id } => {
                    commands::api_keys::run_get(&targets, &id, output).await?;
                }
                ApiKeysCmd::Create { from_file } => {
                    commands::api_keys::run_create(&targets, &from_file, output).await?;
                }
                ApiKeysCmd::Update { from_file, id } => {
                    commands::api_keys::run_update(&targets, &id, &from_file, output).await?;
                }
                ApiKeysCmd::Delete { id } => {
                    commands::api_keys::run_delete(&targets, &id).await?;
                }
                ApiKeysCmd::SendDataKeys => {
                    commands::api_keys::run_send_data_keys(&targets, output).await?;
                }
                ApiKeysCmd::Admin { cmd } => match cmd {
                    ApiKeysAdminCmd::List => {
                        commands::api_keys::run_admin_list(&targets, output).await?;
                    }
                    ApiKeysAdminCmd::Delete { ids } => {
                        commands::api_keys::run_admin_delete(&targets, &ids).await?;
                    }
                    ApiKeysAdminCmd::SetStatus { ids, active } => {
                        commands::api_keys::run_admin_set_status(&targets, &ids, active).await?;
                    }
                },
            },
            IamCmd::Roles { cmd } => match cmd {
                RolesCmd::List => {
                    commands::roles::run_list(&targets, output).await?;
                }
                RolesCmd::Get { id } => {
                    commands::roles::run_get(&targets, &id, output).await?;
                }
                RolesCmd::Create { from_file } => {
                    commands::roles::run_create(&targets, &from_file, output).await?;
                }
                RolesCmd::Update { from_file, id } => {
                    commands::roles::run_update(&targets, &id, &from_file, output).await?;
                }
                RolesCmd::Delete { id } => {
                    commands::roles::run_delete(&targets, &id).await?;
                }
                RolesCmd::System => {
                    commands::roles::run_system(&targets, output).await?;
                }
            },
            IamCmd::Scopes { cmd } => match cmd {
                ScopesCmd::List => {
                    commands::scopes::run_list(&targets, output).await?;
                }
                ScopesCmd::Get { id } => {
                    commands::scopes::run_get(&targets, &id, output).await?;
                }
                ScopesCmd::Create { from_file } => {
                    commands::scopes::run_create(&targets, &from_file, output).await?;
                }
                ScopesCmd::Update { from_file } => {
                    commands::scopes::run_update(&targets, &from_file, output).await?;
                }
                ScopesCmd::Delete { id } => {
                    commands::scopes::run_delete(&targets, &id).await?;
                }
            },
            IamCmd::Users { cmd } => match cmd {
                UsersCmd::Search {
                    query,
                    status,
                    page_size,
                    page_token,
                } => {
                    commands::users::run_search(
                        &targets,
                        query.as_deref(),
                        status.as_deref(),
                        page_size.as_deref(),
                        page_token.as_deref(),
                        output,
                    )
                    .await?;
                }
                UsersCmd::Get { user_id } => {
                    commands::users::run_get(&targets, &user_id, output).await?;
                }
                UsersCmd::Create { from_file } => {
                    commands::users::run_create(&targets, &from_file, output).await?;
                }
                UsersCmd::Update { from_file } => {
                    commands::users::run_update(&targets, &from_file, output).await?;
                }
                UsersCmd::SetStatus { user_ids, status } => {
                    commands::users::run_set_status(&targets, &user_ids, &status).await?;
                }
            },
            IamCmd::TeamGroups { cmd } => match cmd {
                TeamGroupsCmd::List {} => {
                    commands::team_groups::run_list(&targets, output).await?;
                }
                TeamGroupsCmd::Get { id } => {
                    commands::team_groups::run_get(&targets, &id, output).await?;
                }
                TeamGroupsCmd::GetByName { name } => {
                    commands::team_groups::run_get_by_name(&targets, &name, output).await?;
                }
                TeamGroupsCmd::Users { group_id } => {
                    commands::team_groups::run_users(&targets, &group_id, output).await?;
                }
                TeamGroupsCmd::Create { from_file } => {
                    commands::team_groups::run_create(&targets, &from_file, output).await?;
                }
                TeamGroupsCmd::Update { from_file, id } => {
                    commands::team_groups::run_update(&targets, &id, &from_file, output).await?;
                }
                TeamGroupsCmd::Delete { id } => {
                    commands::team_groups::run_delete(&targets, &id).await?;
                }
            },
            IamCmd::Saml { cmd } => match cmd {
                SamlCmd::Get => {
                    commands::saml::run_get(&targets, output).await?;
                }
                SamlCmd::SpParams => {
                    commands::saml::run_sp_params(&targets, output).await?;
                }
                SamlCmd::SetIdp { from_file } => {
                    commands::saml::run_set_idp(&targets, &from_file, output).await?;
                }
                SamlCmd::SetActive { active } => {
                    commands::saml::run_set_active(&targets, active).await?;
                }
            },
            IamCmd::IpAccess { cmd } => match cmd {
                IpAccessCmd::Get => {
                    commands::ip_access::run_get(&targets, output).await?;
                }
                IpAccessCmd::Create { from_file } => {
                    commands::ip_access::run_create(&targets, &from_file, output).await?;
                }
                IpAccessCmd::Update { from_file } => {
                    commands::ip_access::run_update(&targets, &from_file, output).await?;
                }
                IpAccessCmd::Delete => {
                    commands::ip_access::run_delete(&targets).await?;
                }
            },
        },

        Commands::DataArchive { cmd } => match cmd {
            DataArchiveCmd::Metrics { cmd } => match cmd {
                DataArchiveMetricsCmd::Get => {
                    commands::data_archive::run_metrics_get(&targets, output).await?;
                }
                DataArchiveMetricsCmd::Create { from_file } => {
                    commands::data_archive::run_metrics_create(&targets, &from_file, output)
                        .await?;
                }
                DataArchiveMetricsCmd::Update { from_file } => {
                    commands::data_archive::run_metrics_update(&targets, &from_file, output)
                        .await?;
                }
                DataArchiveMetricsCmd::Enable => {
                    commands::data_archive::run_metrics_enable(&targets).await?;
                }
                DataArchiveMetricsCmd::Disable => {
                    commands::data_archive::run_metrics_disable(&targets).await?;
                }
                DataArchiveMetricsCmd::Validate { from_file } => {
                    commands::data_archive::run_metrics_validate(&targets, &from_file, output)
                        .await?;
                }
            },
            DataArchiveCmd::Logs { cmd } => match cmd {
                DataArchiveLogsCmd::Get => {
                    commands::data_archive::run_logs_get(&targets, output).await?;
                }
                DataArchiveLogsCmd::Set { from_file } => {
                    commands::data_archive::run_logs_set(&targets, &from_file, output).await?;
                }
            },
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
