use std::ffi::OsStr;
use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::parser::ValueSource;
use clap::{ArgGroup, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::aot::Shell;
use clap_complete::engine::ArgValueCompleter;
use clap_complete::env::CompleteEnv;
use clap_complete::CompletionCandidate;
use config::OutputFormat;

use coralogix_cli::banner;
use coralogix_cli::commands;
use coralogix_cli::commands::dataprime::DataprimeFilter;
use coralogix_cli::config;
use coralogix_cli::execution::build_targets;
use coralogix_cli::safety;
use coralogix_cli::safety::confirm_destructive;
use coralogix_cli::update_check;
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

/// Value parser for `cx init --install-completions <shell>`. Restricts the
/// choice to the shells the guided flow can install to a known default path
/// (zsh, bash, fish); other shells need an explicit path, available via the
/// interactive picker's "Other" option or `cx completions install --path`.
fn parse_completions_shell(value: &str) -> Result<Shell, String> {
    match value {
        "zsh" => Ok(Shell::Zsh),
        "bash" => Ok(Shell::Bash),
        "fish" => Ok(Shell::Fish),
        other => Err(format!(
            "unsupported shell '{other}' (choose zsh, bash, or fish; \
             for other shells use `cx completions install <shell> --path ...`)"
        )),
    }
}

/// How `search-fields` searches: by semantic description or by value content.
#[derive(Debug, Clone, ValueEnum, Default)]
pub enum SearchType {
    /// Semantic / description-based field search (default).
    #[default]
    Semantic,
    /// Search for fields by value content.
    Value,
}

/// Dataset for `search-fields`. `all` is only valid with `--search-type value`.
#[derive(Debug, Clone, ValueEnum)]
pub enum SearchByValueDataset {
    Logs,
    Spans,
    All,
}

/// Coralogix CLI - the observability backbone for AI agents and engineering teams.
#[derive(Parser)]
#[command(
    name = "cx",
    version,
    about,
    long_about = None,
    help_template = "{before-help}{about-with-newline}\n{usage-heading} {usage}{after-help}\n\n\x1b[1m\x1b[4mGlobal Options:\x1b[0m\n{options}",
    after_help = "\
\x1b[1m\x1b[4mQuery:\x1b[0m
  \x1b[1mlogs\x1b[0m               Query logs using DataPrime syntax
  \x1b[1mspans\x1b[0m              Query spans using DataPrime syntax
  \x1b[1mmetrics\x1b[0m            Query metrics using PromQL
  \x1b[1mdataprime\x1b[0m          DataPrime language reference and raw queries
  \x1b[1mdocs\x1b[0m               Search and fetch official Coralogix product documentation
  \x1b[1msearch-fields\x1b[0m      Search log/span fields by description or value content

\x1b[1m\x1b[4mObserve:\x1b[0m
  \x1b[1mdashboards\x1b[0m         Manage dashboards and dashboard folders
  \x1b[1mviews\x1b[0m              Manage saved views and view folders
  \x1b[1mslos\x1b[0m               Manage SLO definitions
  \x1b[1minfra\x1b[0m              Query infrastructure resources and their data
  \x1b[1mservice-catalog\x1b[0m    Query service-catalog entities and their RED/health/saturation data

\x1b[1m\x1b[4mAI:\x1b[0m
  \x1b[1mai-center\x1b[0m (risky)  Manage AI Center applications, evaluations, policies, and pricing

\x1b[1m\x1b[4mDetect & Respond:\x1b[0m
  \x1b[1malerts\x1b[0m             Manage alert definitions and suppression rules
  \x1b[1mcases\x1b[0m               Manage and triage cases

\x1b[1m\x1b[4mNotifications:\x1b[0m
  \x1b[1mnotifications\x1b[0m      Manage connectors, routers, presets, and notification testing
  \x1b[1mwebhooks\x1b[0m           Manage outgoing webhooks and automation actions

\x1b[1m\x1b[4mData Pipeline:\x1b[0m
  \x1b[1mparsing-rules\x1b[0m      Manage log parsing rules
  \x1b[1menrichments\x1b[0m        Manage enrichment rules and custom enrichment tables
  \x1b[1me2m\x1b[0m                Manage Events2Metrics definitions
  \x1b[1mrecording-rules\x1b[0m    Manage Prometheus recording rule groups

\x1b[1m\x1b[4mCost & Storage:\x1b[0m
  \x1b[1musage\x1b[0m              View data usage and consumption metrics
  \x1b[1mtco\x1b[0m                Manage TCO policies and settings
  \x1b[1mretentions\x1b[0m         Manage data retention settings
  \x1b[1marchive\x1b[0m (risky)    Manage data archive storage configuration

\x1b[1m\x1b[4mIntegrations:\x1b[0m
  \x1b[1mintegrations\x1b[0m       Manage integrations, extensions, and contextual data

\x1b[1m\x1b[4mAccess:\x1b[0m
  \x1b[1miam\x1b[0m (risky)        Manage API keys, roles, scopes, users, groups, and IP access

\x1b[1m\x1b[4mAgent:\x1b[0m
  \x1b[1mschema\x1b[0m             Output the full command tree as JSON for agent consumption
  \x1b[1molly\x1b[0m               Interact with the AI assistant

\x1b[1m\x1b[4mLocal:\x1b[0m
  \x1b[1minit\x1b[0m               One-step onboarding: configure a profile and install the agent skills
  \x1b[1mprofiles\x1b[0m           Manage profiles (list, add, refresh, delete, set-default)
  \x1b[1mskills\x1b[0m             Install or update the cx agent skills for coding agents
  \x1b[1mcleanup\x1b[0m            Remove stale temp files"
)]
struct Cli {
    /// Profile(s) to use. Repeat to fan out across multiple profiles simultaneously.
    /// Overrides the default profile set in config.
    #[arg(
        long,
        short = 'p',
        global = true,
        env = "CX_PROFILE",
        help_heading = "Global Options",
        add = ArgValueCompleter::new(complete_profile_names)
    )]
    profile: Vec<String>,

    /// Coralogix API key (overrides a single profile; incompatible with multiple --profile).
    #[arg(
        long,
        global = true,
        env = "CX_API_KEY",
        help_heading = "Global Options"
    )]
    api_key: Option<String>,

    /// Coralogix region (overrides a single profile; incompatible with multiple --profile).
    #[arg(
        long,
        global = true,
        env = "CX_REGION",
        help_heading = "Global Options"
    )]
    region: Option<String>,

    /// Output format: text, json, or toon. Overrides the default set in config.
    #[arg(long, short = 'o', global = true, help_heading = "Global Options")]
    output: Option<OutputFormat>,

    /// Skip confirmation prompts for destructive operations.
    #[arg(long, global = true, help_heading = "Global Options")]
    yes: bool,

    /// Block all write operations. Useful for safe agent/automation access.
    #[arg(long, global = true, help_heading = "Global Options")]
    read_only: bool,

    /// Suppress "View in Coralogix" console links (stderr line).
    #[arg(long, global = true, help_heading = "Global Options")]
    no_console_link: bool,

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
    /// Manage profiles (list, add, refresh, delete, set-default).
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
enum SkillsCmd {
    /// Install or update the cx agent skills bundle via the `skills` npx installer.
    ///
    /// By default this asks one question (install scope) and then runs the
    /// installer fully non-interactively with agent auto-detection. Re-running
    /// updates already-installed skills to the latest published bundle.
    /// Requires Node.js (npx).
    #[command(after_help = "\
Examples:
  cx skills install                     # asks global vs local, then installs
  cx skills install --global            # no questions asked (also updates in place)
  cx skills install --local --agent claude-code
  cx skills install --interactive       # walk the installer's full flow")]
    Install {
        /// Install skills globally (~/), available in every project.
        #[arg(long, conflicts_with_all = ["local", "interactive"])]
        global: bool,

        /// Install skills locally (./), for this project only.
        #[arg(long, conflicts_with = "interactive")]
        local: bool,

        /// Target specific agents (passed through to the installer's -a;
        /// overrides its auto-detection). Repeatable.
        #[arg(long = "agent", value_name = "NAME", conflicts_with = "interactive")]
        agents: Vec<String>,

        /// Walk the skills installer's full interactive flow (skill/agent
        /// selection, scope, install method) instead of the default
        /// non-interactive install.
        #[arg(long)]
        interactive: bool,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// One-step onboarding: configure a profile and install the cx agent skills.
    ///
    /// Interactive by default (OAuth browser login); pass `--api-key` (or set
    /// CX_API_KEY) to authenticate with an API key instead. With `--url` and
    /// an API key the profile step is prompt-free; add `--global-skills`/
    /// `--local-skills` (or `--no-skills`) to also answer the skills-scope
    /// question and get a fully
    /// prompt-free run for CI and coding agents — without a scope flag, a run
    /// with no terminal skips the skills install with a warning. `--oauth`
    /// works without a terminal too: the sign-in URL is printed for you (or an
    /// agent's user) to approve in a browser, then the command waits for the
    /// approval — handy when no API key is on hand, but because it needs a
    /// browser it is not suited to fully headless CI (use `--api-key` there).
    /// Idempotent: if a profile already exists the profile step is skipped
    /// (reconfigure with `cx profiles add --force`).
    #[command(after_help = "\
Examples:
  cx init                                              # interactive walkthrough
  cx init --url https://myteam.app.eu2.coralogix.com --api-key $CX_API_KEY --global-skills
  cx init --oauth --url https://myteam.app.eu2.coralogix.com
  cx init --no-skills                                  # skip the agent-skills install")]
    Init {
        /// Coralogix URL to derive the region from (e.g. your browser URL).
        /// Unrecognized URLs are used as a custom API endpoint (BYOC / private link).
        #[arg(long)]
        url: Option<String>,
        /// Force OAuth browser login, ignoring any supplied API key
        /// (--api-key / CX_API_KEY). Without a key, OAuth is used anyway.
        /// No terminal required: the sign-in URL is printed and the command
        /// waits while it is approved in a browser, so an agent can onboard
        /// with OAuth by surfacing the URL to its user.
        #[arg(long)]
        oauth: bool,
        /// Skip the agent-skills install step (installed by default).
        #[arg(long, conflicts_with_all = ["global_skills", "local_skills", "agents"])]
        no_skills: bool,
        /// Install skills globally (~/), available in every project.
        #[arg(long, conflicts_with = "local_skills")]
        global_skills: bool,
        /// Install skills locally (./), for this project only.
        #[arg(long)]
        local_skills: bool,
        /// Target specific agents for the skills install (passed through to the
        /// installer's -a; overrides its auto-detection). Repeatable.
        #[arg(long = "agent", value_name = "NAME")]
        agents: Vec<String>,
        /// Install shell completions for the given shell (zsh, bash, or fish)
        /// without prompting. Omit to be asked interactively (a picker with a
        /// "don't install" default); a non-interactive run then skips the step.
        /// Ignored when completions are already installed - use
        /// `cx completions install <shell>` to add a shell or reinstall.
        #[arg(long, value_name = "SHELL", value_parser = parse_completions_shell)]
        install_completions: Option<Shell>,
    },

    /// Manage profiles (list, add, refresh, delete, set-default).
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

    /// Install or update the cx agent skills for coding agents (Claude Code, Cursor, Codex, ...).
    ///
    /// Re-run `cx skills install` anytime to update already-installed skills
    /// to the latest published bundle.
    Skills {
        #[command(subcommand)]
        cmd: SkillsCmd,
    },

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

        /// Storage tier to search. Overrides the profile's default_tier setting.
        /// If neither is set, defaults to archive.
        #[arg(long)]
        tier: Option<Tier>,
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

        /// Storage tier to search. Overrides the profile's default_tier setting.
        /// If neither is set, defaults to archive.
        #[arg(long)]
        tier: Option<Tier>,
    },

    /// Manage and inspect dashboards.
    Dashboards {
        #[command(subcommand)]
        cmd: DashboardsCmd,
    },

    /// Manage alert definitions and suppression rules.
    Alerts {
        #[command(subcommand)]
        cmd: AlertsCmd,
    },

    /// Manage and triage cases.
    #[command(after_help = "\
Examples:
  cx cases get <case-id>
  cx cases assign <case-id> --user alice@example.com
  cx cases acknowledge <case-id>
  cx cases resolve <case-id> --reason \"Mitigated by rollback\"
  cx cases close <case-id>
  cx cases comment <case-id> --text \"Investigating the root cause\"
  cx cases events list <case-id>
  cx cases notifications <case-id>")]
    Cases {
        #[command(subcommand)]
        cmd: CasesCmd,
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
  cx usage spans-count --start now-24h --end now
  cx usage capabilities
  cx usage query --from-file query.json"
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

    /// Manage log parsing rules.
    #[command(
        name = "parsing-rules",
        after_help = "\
Examples:
  cx parsing-rules list
  cx parsing-rules get <group-id>
  cx parsing-rules create --from-file group.json
  cx parsing-rules update --from-file group.json <group-id>
  cx parsing-rules delete <group-id>
  cx parsing-rules usage-limits"
    )]
    ParsingRules {
        #[command(subcommand)]
        cmd: ParsingRulesCmd,
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

    /// Manage API keys, roles, scopes, users, groups, and IP access.
    #[command(after_help = "\
Examples:
  cx iam api-keys list
  cx iam roles list
  cx iam scopes list
  cx iam users search
  cx iam groups list
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

    /// Manage AI Center (GenAI) applications, evaluations, policies, and pricing.
    #[command(
        name = "ai-center",
        after_help = "\
Examples:
  cx ai-center applications list
  cx ai-center evaluations list --application <app> --subsystem <sub>
  cx ai-center coverage
  cx ai-center custom-evaluations list
  cx ai-center model-pricing get"
    )]
    AiCenter {
        #[command(subcommand)]
        cmd: AiCenterCmd,
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

    /// Query infrastructure resources and their data.
    #[command(after_help = "\
Examples:
  cx infra resources types
  cx infra resources list --category Hosts --type EC2_Instances")]
    Infra {
        #[command(subcommand)]
        cmd: InfraCmd,
    },

    /// Query service-catalog v2 entities: RED metrics, health, resource
    /// saturation (k8s-pod/jvm), and dependency columns.
    #[command(after_help = "\
Examples:
  cx service-catalog entity-types
  cx service-catalog schema service
  cx service-catalog entities service
  cx service-catalog data service --start now-1h --end now --column latency_p99
  cx service-catalog entity-data service checkout --start now-1h --end now --column latency_p99")]
    ServiceCatalog {
        #[command(subcommand)]
        cmd: ServiceCatalogCmd,
    },

    /// Search log/span fields by description or by value content.
    #[command(after_help = "\
Examples:
  cx search-fields \"http response status code\"
  cx search-fields \"error severity level\" --dataset spans --limit 10
  cx search-fields \"payment\" -s value --dataset logs
  cx search-fields \"kubernetes pod\" -s value --dataset all --limit 20 --offset 10")]
    SearchFields {
        /// Description or value to search for.
        text: String,

        /// Search type: semantic (description-based, default) or value (by field value content).
        #[arg(short = 's', long, default_value = "semantic")]
        search_type: SearchType,

        /// Dataset to search: logs or spans (semantic); logs, spans, or all (value).
        #[arg(long, default_value = "logs")]
        dataset: SearchByValueDataset,

        /// Maximum number of results to return.
        #[arg(long, default_value_t = 10)]
        limit: u32,

        /// Number of results to skip for pagination (only used with -s value).
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },

    /// DataPrime language reference and documentation.
    Dataprime {
        #[command(subcommand)]
        cmd: DataprimeCmd,
    },

    /// Search and fetch official Coralogix product documentation (not live tenant data).
    Docs {
        #[command(subcommand)]
        cmd: DocsCmd,
    },

    /// Output the full command tree as JSON for agent consumption.
    Schema,

    /// Interact with the AI assistant (single-profile only).
    #[command(after_help = "\
Examples:
  cx olly ask \"What alerts fired today?\"
  cx olly ask \"Show me error logs\" --chat-id <id>
  cx olly ask \"Analyze this metric\" --model claude-sonnet-4-5
  cx olly artifacts get <artifact-id>")]
    Olly {
        #[command(subcommand)]
        cmd: OllyCmd,
    },
}

impl Commands {
    fn is_risky(&self) -> bool {
        matches!(
            self,
            Self::Iam { .. } | Self::DataArchive { .. } | Self::AiCenter { .. }
        )
    }

    fn is_olly(&self) -> bool {
        match self {
            Self::Olly { cmd } => matches!(cmd, OllyCmd::Ask { .. }),
            _ => false,
        }
    }
}

#[derive(Subcommand)]
enum ProfilesCmd {
    /// List all configured profiles.
    List,
    /// Add or reconfigure a profile.
    ///
    /// Values supplied via flags/env are never prompted for. On a terminal,
    /// missing values are prompted interactively. Without a terminal (or when
    /// both an API key and a region/URL are supplied), nothing is prompted:
    /// missing required values are errors, and existing profiles are only
    /// overwritten with --force.
    #[command(after_help = "\
Examples:
  cx profiles add                                        # fully interactive
  cx profiles add prod --region eu2                      # region answered, rest prompted
  cx profiles add --oauth --region eu2                   # straight to browser login
  cx profiles add --url https://myteam.app.eu2.coralogix.com --api-key $KEY
  CX_API_KEY=$KEY cx profiles add --region us1 --force   # non-interactive overwrite")]
    Add {
        /// Profile name to configure (prompted if not provided; defaults to
        /// "default" when running non-interactively).
        #[arg(add = ArgValueCompleter::new(complete_profile_names))]
        name: Option<String>,
        /// Profile name to configure (alternative to the positional NAME).
        /// Named --name to stay clear of the global --profile selector.
        #[arg(long = "name", conflicts_with = "name", value_name = "NAME")]
        name_flag: Option<String>,
        /// Coralogix URL to derive the region from (e.g. your browser URL).
        /// Unrecognized URLs are used as a custom API endpoint (BYOC / private link).
        #[arg(long, conflicts_with = "region")]
        url: Option<String>,
        /// Region short-name (us1, us2, us3, eu1, eu2, ap1, ap2, ap3). Alternative to --url.
        #[arg(long)]
        region: Option<String>,
        /// API key (Team Key or Personal Key). Also read from CX_API_KEY.
        #[arg(long, env = "CX_API_KEY", hide_env_values = true, value_name = "KEY")]
        api_key: Option<String>,
        /// Use OAuth browser login, skipping the auth-method prompt. Takes
        /// precedence over --api-key / CX_API_KEY. Prints the sign-in URL, so
        /// it also works without a terminal (requires --url or --region there).
        #[arg(long)]
        oauth: bool,
        /// Overwrite an existing profile without prompting.
        #[arg(long)]
        force: bool,
        /// Set this profile as the default without prompting.
        #[arg(long)]
        set_default: bool,
        /// When creating the first profile, disable the Olly AI assistant
        /// (`cx olly ask`). Olly is enabled by default; this opts out. Only
        /// affects first-profile setup, where the global Olly setting is
        /// written. No prompt either way.
        #[arg(long)]
        disable_olly: bool,
    },
    /// Re-run the OAuth browser login for an existing profile.
    ///
    /// Replaces only the stored OAuth tokens. Region, label, credential
    /// storage, and output format are left exactly as they are, and nothing
    /// is prompted for. Use this when a session has expired.
    Refresh {
        /// Profile name to re-authenticate.
        #[arg(add = ArgValueCompleter::new(complete_profile_names))]
        name: String,
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
enum DocsCmd {
    /// Search official Coralogix docs by title or path.
    #[command(after_help = "\
Examples:
  cx docs search \"explore spans\" --limit 5
  cx docs search \"OpenTelemetry traces\"")]
    Search {
        /// Search text (short keywords work best).
        query: String,

        /// Maximum number of results (1–20).
        #[arg(long, default_value_t = 5)]
        limit: u32,
    },

    /// Fetch one Coralogix docs page as markdown.
    #[command(after_help = "\
Example:
  cx docs fetch user-guides/data_exploration/spans/")]
    Fetch {
        /// Path suffix from `cx docs search` (not a full URL).
        suffix: String,
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

        /// Storage tier to search. Overrides the profile's default_tier setting.
        /// If neither is set, defaults to archive.
        #[arg(long)]
        tier: Option<Tier>,
    },
}

#[derive(Subcommand)]
enum OllyCmd {
    /// Send a message to the AI assistant.
    #[command(after_help = "\
Examples:
  cx olly ask \"What alerts fired today?\"
  cx olly ask \"Show me error logs\" --chat-id <uuid>
  cx olly ask \"Analyze this\" --model claude-sonnet-4-5
  cx olly ask \"Find error logs that start with 'Cart Not Found' in the last 6 hours\" --agent-to-agent-mode")]
    Ask {
        /// The message to send to the assistant.
        message: String,

        /// Continue an existing chat (omit to create a new chat).
        #[arg(long)]
        chat_id: Option<String>,

        /// Model choice (e.g., gpt-5.2, claude-sonnet-4-5, gpt-5.4, claude-haiku-4-5).
        #[arg(long, default_value = "gpt-5.2")]
        model: String,

        /// Timeout in seconds for response.
        #[arg(long, default_value_t = 900)]
        timeout: u32,

        /// Ask Olly as a sub-agent (agent-to-agent mode): shorter responses, no
        /// charts/tables, asks clarifying questions instead of guessing.
        /// Defaults to false (human-facing); pass this flag if you're an
        /// LLM/agent calling `cx` to opt into shorter, sub-agent-style responses.
        #[arg(long)]
        agent_to_agent_mode: bool,
    },

    /// Manage artifacts from assistant responses.
    Artifacts {
        #[command(subcommand)]
        cmd: OllyArtifactsCmd,
    },
}

#[derive(Subcommand)]
enum OllyArtifactsCmd {
    /// List all artifacts.
    #[command(after_help = "\
Examples:
  cx olly artifacts list
  cx olly artifacts list -o json")]
    List,

    /// Get an artifact's download URL.
    #[command(after_help = "\
Examples:
  cx olly artifacts get <artifact-id>
  cx olly artifacts get <artifact-id> -o json")]
    Get {
        /// Artifact ID (UUID from the assistant's response).
        artifact_id: String,
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
    /// Create a new dashboard from a JSON definition file [requires --yes].
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
    /// Replace an existing dashboard with an updated JSON definition [requires --yes].
    #[command(after_help = "\
Examples:
  cx dashboards get <id> -o json > dash.json
  # edit dash.json...
  cx dashboards replace --from-file dash.json
  cat dash.json | cx dashboards replace")]
    Replace {
        /// Path to a JSON file with the updated dashboard definition. Use '-' for stdin.
        /// The JSON must include the dashboard `id` field. Accepts either a bare dashboard
        /// document or a `{"dashboard": {...}}` wrapper; the `requestId` envelope field
        /// is generated automatically.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Validate a dashboard definition without persisting it (CheckDashboard).
    ///
    /// Read-only. Exits non-zero if any error-severity issue is found (CI gate).
    /// In multi-profile fan-out, any profile returning error-severity issues
    /// causes a non-zero exit, even if other profiles are clean.
    #[command(after_help = "\
Examples:
  cx dashboards check --from-file dash.json
  cx dashboards check --from-file -            # read from stdin
  cx dashboards check 01234abcd                 # validate a stored dashboard by id
  cx -p prod -p staging dashboards check 01234abcd   # multi-profile")]
    Check {
        /// Path to a JSON file with the dashboard definition. Use '-' for stdin.
        /// Accepts either a bare dashboard document or a `{\"dashboard\": {...}}` wrapper.
        /// Mutually exclusive with <DASHBOARD_ID>.
        #[arg(long, conflicts_with = "dashboard_id")]
        from_file: Option<String>,

        /// Validate an existing dashboard by id. Mutually exclusive with --from-file.
        #[arg(conflicts_with = "from_file")]
        dashboard_id: Option<String>,
    },
    /// Delete a dashboard [requires --yes].
    Delete {
        /// Dashboard ID.
        dashboard_id: String,
    },
    /// Search dashboards semantically by description.
    #[command(after_help = "\
Examples:
  cx dashboards search \"kubernetes pod memory\"
  cx dashboards search \"latency over time\" --limit 5")]
    Search {
        /// Natural-language description to search for.
        description: String,

        /// Maximum number of results to return.
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },

    /// Search dashboard queries by field reference or description.
    #[command(after_help = "\
Examples:
  cx dashboards query-search --field \"$d.http.status\"
  cx dashboards query-search --description \"http error rate\"
  cx dashboards query-search --description \"cpu usage\" --limit 5")]
    QuerySearch {
        /// Find queries that reference a specific field path.
        #[arg(long, conflicts_with = "description")]
        field: Option<String>,

        /// Find queries matching a natural-language description.
        #[arg(long, conflicts_with = "field")]
        description: Option<String>,

        /// Maximum number of results to return.
        #[arg(long, default_value_t = 10)]
        limit: u32,
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
    /// Create a new dashboard folder [requires --yes].
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
    /// Delete a dashboard folder [requires --yes].
    Delete {
        /// Folder ID.
        id: String,
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
    /// Create an alert from a JSON definition file [requires --yes].
    #[command(after_help = "\
Examples:
  cx alerts create --from-file alert.json
  cat alert.json | cx alerts create")]
    Create {
        /// Path to JSON file with the alert definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an alert [requires --yes].
    Delete {
        /// Alert definition ID (UUID).
        alert_id: String,
    },
    /// Enable an alert [requires --yes].
    Enable {
        /// Alert definition ID (UUID).
        alert_id: String,
    },
    /// Disable an alert [requires --yes].
    Disable {
        /// Alert definition ID (UUID).
        alert_id: String,
    },
    /// List alert events (trigger instances).
    #[command(after_help = "\
Examples:
  cx alerts events
  cx alerts events --alert-version-id <version-id>
  cx alerts events --alert-version-id <version-id> --start now-24h")]
    Events {
        /// Filter by alert version ID. Repeat to include multiple alert versions.
        #[arg(long)]
        alert_version_id: Vec<String>,

        /// Start time filter (ISO 8601 or relative).
        #[arg(long)]
        start: Option<String>,

        /// End time filter (ISO 8601 or relative).
        #[arg(long)]
        end: Option<String>,
    },
    /// Get alert event statistics.
    EventStats,
    /// Manage alert suppression rules.
    #[command(after_help = "\
Examples:
  cx alerts suppression-rules list
  cx alerts suppression-rules get <rule-id>
  cx alerts suppression-rules create --from-file rule.json
  cx alerts suppression-rules delete <rule-id>")]
    SuppressionRules {
        #[command(subcommand)]
        cmd: SuppressionRulesCmd,
    },
}

#[derive(Subcommand)]
enum CasesCmd {
    /// Get a single case by ID.
    Get {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
    },
    /// Update mutable fields on a case.
    Update {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
        /// New case title.
        #[arg(long)]
        title: Option<String>,
        /// Resolution reason.
        #[arg(long)]
        resolution_reason: Option<String>,
    },
    /// Add a comment to a case.
    Comment {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
        /// Comment text to add to the case.
        #[arg(long)]
        text: String,
    },
    /// Assign a case to a user.
    Assign {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
        /// User to assign the case to — accepts an email address (resolved via
        /// the team-members directory) or a raw user ID.
        #[arg(long)]
        user: String,
    },
    /// Remove the assignee from a case.
    Unassign {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
    },
    /// Acknowledge a case.
    Acknowledge {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
    },
    /// Remove the acknowledgment from a case.
    Unacknowledge {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
    },
    /// Resolve a case [irreversible — requires confirmation; pass --yes to skip prompts].
    Resolve {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
        /// Resolution reason (required unless --no-reason is passed; prompted if
        /// omitted in an interactive terminal).
        #[arg(long, conflicts_with = "no_reason")]
        reason: Option<String>,
        /// Resolve without a reason. Use only when a reason genuinely does not
        /// apply — an empty resolution loses the audit trail.
        #[arg(long)]
        no_reason: bool,
    },
    /// Close a case.
    Close {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
    },
    /// Override a case's computed priority.
    SetPriority {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
        /// Priority to set (e.g. P1, P2, P3, P4, P5).
        #[arg(long)]
        priority: String,
    },
    /// Clear a previously set priority override.
    ClearPriority {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        id: String,
    },
    /// Inspect the event timeline of a case.
    #[command(after_help = "\
Examples:
  cx cases events list <case-id>
  cx cases events get <event-id>")]
    Events {
        #[command(subcommand)]
        cmd: CasesEventsCmd,
    },
    /// List notification deliveries for one or more cases.
    #[command(after_help = "\
Examples:
  cx cases notifications <case-id>
  cx cases notifications <case-id-1> <case-id-2>")]
    Notifications {
        /// One or more case IDs (UUIDs).
        #[arg(required = true)]
        case_ids: Vec<String>,
    },
}

#[derive(Subcommand)]
enum CasesEventsCmd {
    /// List all events on a case (status changes, comments, etc.).
    List {
        /// Case ID (UUID or readable ID, e.g. CASE-123).
        case_id: String,
    },
    /// Get a single case event by its event ID.
    Get {
        /// Event ID (UUID).
        event_id: String,
    },
}

#[derive(Subcommand)]
enum SuppressionRulesCmd {
    /// List all suppression rules.
    List,
    /// Get a single suppression rule by ID.
    Get {
        /// Suppression rule ID.
        id: String,
    },
    /// Create a suppression rule from a JSON definition file [requires --yes].
    #[command(after_help = "\
Examples:
  cx alerts suppression-rules create --from-file rule.json
  cat rule.json | cx alerts suppression-rules create")]
    Create {
        /// Path to JSON file with the rule definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a suppression rule from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated rule definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a suppression rule [requires --yes].
    Delete {
        /// Suppression rule ID.
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
    /// Create a connector from a JSON definition file [requires --yes].
    Create {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a connector definition from a JSON file [requires --yes].
    Update {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a connector [requires --yes].
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
    /// Create a router from a JSON definition file [requires --yes].
    Create {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a router definition from a JSON file [requires --yes].
    Update {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a router [requires --yes].
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
    /// Create a custom preset from a JSON definition file [requires --yes].
    Create {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a custom preset from a JSON file [requires --yes].
    Update {
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a custom preset [requires --yes].
    Delete { id: String },
    /// Set default preset [requires --yes].
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
    #[command(after_help = "\
Examples:
  cx usage logs-count
  cx usage logs-count --start now-7d --end now --resolution 6h
  cx usage logs-count --application-aggregation

Output:
  JSON output is normalized to one object with rows under result.logsCount.
  Large backend responses may arrive in multiple JSON chunks; cx merges them.")]
    LogsCount {
        /// Start time filter (ISO 8601 or relative, e.g. now-7d). Defaults to 24h ago.
        #[arg(long)]
        start: Option<String>,

        /// End time filter (ISO 8601 or relative, e.g. now). Defaults to now.
        #[arg(long)]
        end: Option<String>,

        /// Query resolution. Defaults to 1h.
        #[arg(long)]
        resolution: Option<String>,

        /// Aggregate by subsystem.
        #[arg(long)]
        subsystem_aggregation: bool,

        /// Aggregate by application.
        #[arg(long)]
        application_aggregation: bool,

        /// Extra raw query parameter in KEY=VALUE form. Repeat for filters.* fields.
        #[arg(long = "param")]
        params: Vec<String>,
    },
    /// Show spans count.
    #[command(after_help = "\
Examples:
  cx usage spans-count
  cx usage spans-count --start now-7d --end now --resolution 6h
  cx usage spans-count --application-aggregation

Output:
  JSON output is normalized to one object with rows under result.spansCount.
  Large backend responses may arrive in multiple JSON chunks; cx merges them.")]
    SpansCount {
        /// Start time filter (ISO 8601 or relative, e.g. now-7d). Defaults to 24h ago.
        #[arg(long)]
        start: Option<String>,

        /// End time filter (ISO 8601 or relative, e.g. now). Defaults to now.
        #[arg(long)]
        end: Option<String>,

        /// Query resolution. Defaults to 1h.
        #[arg(long)]
        resolution: Option<String>,

        /// Aggregate by subsystem.
        #[arg(long)]
        subsystem_aggregation: bool,

        /// Aggregate by application.
        #[arg(long)]
        application_aggregation: bool,

        /// Extra raw query parameter in KEY=VALUE form. Repeat for filters.* fields.
        #[arg(long = "param")]
        params: Vec<String>,
    },
    /// Show the labels, measurements, and limits supported by the Data Usage Query API.
    Capabilities,
    /// Query billable data usage using a capabilities-derived JSON request.
    #[command(
        group(
            ArgGroup::new("query_input")
                .required(true)
                .args(["from_file", "query"])
        ),
        after_help = "\
Workflow:
  1. Run `cx usage capabilities -o json`.
  2. Build a request using only the returned labels, measurements, and limits.

Examples:
  cx usage query --from-file query.json
  cx usage query --query '{\"daily\":{\"relativeRange\":\"DAILY_RELATIVE_RANGE_LAST_7_DAYS\"}}'
  cat query.json | cx usage query --from-file -"
    )]
    Query {
        /// Path to a JSON query request. Use '-' for stdin. Mutually exclusive with --query.
        #[arg(long, conflicts_with = "query")]
        from_file: Option<String>,

        /// Inline JSON query request. Mutually exclusive with --from-file.
        #[arg(long, conflicts_with = "from_file")]
        query: Option<String>,
    },
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
    /// Create a TCO policy from a JSON definition file [requires --yes].
    Create {
        /// Path to JSON file with the policy definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a TCO policy from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated policy definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a TCO policy [requires --yes].
    Delete {
        /// TCO policy ID.
        id: String,
    },
    /// Reorder TCO policies by priority [requires --yes].
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
    /// Replace TCO settings from a JSON file [requires --yes].
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
    /// Update retention settings from a JSON file [requires --yes].
    Update {
        /// Path to JSON file with retention settings. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Activate retention [requires --yes].
    Activate,
    /// Check retention enabled status.
    Status,
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
    /// Create an E2M definition from a JSON file [requires --yes].
    Create {
        /// Path to JSON file with the E2M definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace an E2M definition from a JSON file [requires --yes].
    Update {
        /// Path to JSON file with the updated E2M definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an E2M definition [requires --yes].
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
    /// Create a recording rule group from a JSON definition file [requires --yes].
    Create {
        /// Path to JSON file with the rule group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a recording rule group from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated rule group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,

        /// Recording rule group ID.
        id: String,
    },
    /// Delete a recording rule group [requires --yes].
    Delete {
        /// Recording rule group ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum ParsingRulesCmd {
    /// List all parsing rule groups.
    List,
    /// Get a single parsing rule group by ID.
    Get {
        /// Parsing rule group ID.
        id: String,
    },
    /// Create a parsing rule group from a JSON definition file [requires --yes].
    Create {
        /// Path to JSON file with the parsing rule group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a parsing rule group from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated parsing rule group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// Parsing rule group ID.
        id: String,
    },
    /// Delete a parsing rule group [requires --yes].
    Delete {
        /// Parsing rule group ID.
        id: String,
    },
    /// Bulk delete parsing rule groups by IDs [requires --yes].
    BulkDelete {
        /// Parsing rule group IDs to delete.
        #[arg(long, num_args = 1..)]
        ids: Vec<String>,
    },
    /// Show parsing rule usage limits.
    UsageLimits,
}

#[derive(Subcommand)]
enum EnrichmentsCmd {
    /// List all enrichment rules.
    List,
    /// Add enrichment rules from a JSON file [requires --yes].
    Add {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Remove enrichment rules from a JSON file [requires --yes].
    Remove {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Overwrite all enrichment rules from a JSON file [requires --yes].
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
    /// Create a custom enrichment table from a JSON file [requires --yes].
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a custom enrichment table from a JSON file [requires --yes].
    Update {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a custom enrichment table [requires --yes].
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
    /// Create an integration from a JSON file [requires --yes].
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update an integration from a JSON file [requires --yes].
    Update {
        /// Integration ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an integration [requires --yes].
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
    /// Deploy an extension from a JSON file [requires --yes].
    Deploy {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a deployed extension from a JSON file [requires --yes].
    Update {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Undeploy an extension from a JSON file [requires --yes].
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
    /// Create a webhook from a JSON file [requires --yes].
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a webhook from a JSON file [requires --yes].
    Update {
        /// Webhook ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a webhook [requires --yes].
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
    /// Create an integration from a JSON file [requires --yes].
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update an integration from a JSON file [requires --yes].
    Update {
        /// Integration ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an integration [requires --yes].
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
    /// Create a view from a JSON file [requires --yes].
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a view from a JSON file [requires --yes].
    Update {
        /// View ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a view [requires --yes].
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
    /// Create a folder from a JSON file [requires --yes].
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace a folder from a JSON file [requires --yes].
    Update {
        /// Folder ID.
        id: String,
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a folder [requires --yes].
    Delete {
        /// Folder ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum AiCenterCmd {
    /// Manage AI applications (inventory + guarded status).
    #[command(after_help = "\
Examples:
  cx ai-center applications list
  cx ai-center applications get <application-id>")]
    Applications {
        #[command(subcommand)]
        cmd: ApplicationsCmd,
    },
    /// Manage configured evaluations/policies on applications.
    #[command(after_help = "\
Examples:
  cx ai-center evaluations list --application <app> --subsystem <sub>
  cx ai-center evaluations get <evaluation-id>
  cx ai-center evaluations create --from-file eval.json
  cx ai-center evaluations update <evaluation-id> --from-file eval.json
  cx ai-center evaluations delete <evaluation-id>")]
    Evaluations {
        #[command(subcommand)]
        cmd: EvaluationsCmd,
    },
    /// Manage custom evaluation policies and their application links.
    #[command(after_help = "\
Examples:
  cx ai-center custom-evaluations list
  cx ai-center custom-evaluations list-for-application <application-id>
  cx ai-center custom-evaluations create --from-file policy.json
  cx ai-center custom-evaluations add <evaluation-id> <application-id>
  cx ai-center custom-evaluations remove <evaluation-id> <application-id>")]
    CustomEvaluations {
        #[command(subcommand)]
        cmd: CustomEvaluationsCmd,
    },
    /// Show evaluation coverage — AI applications per evaluation type.
    Coverage,
    /// View and set the team's custom model-pricing overrides.
    #[command(after_help = "\
Examples:
  cx ai-center model-pricing get
  cx ai-center model-pricing set --from-file pricing.json")]
    ModelPricing {
        #[command(subcommand)]
        cmd: ModelPricingCmd,
    },
}

#[derive(Subcommand)]
enum ApplicationsCmd {
    /// List AI applications (incl. guarded status).
    List {
        /// Maximum number of applications to return.
        #[arg(long)]
        page_size: Option<u32>,
        /// Number of applications to skip for pagination.
        #[arg(long)]
        page_offset: Option<u32>,
        /// Filter to apps using this evaluation type, as the API enum (e.g. PII,
        /// TOXICITY, PROMPT_INJECTION — the keys from `coverage`). Repeatable.
        #[arg(long = "evaluation-type")]
        evaluation_type: Vec<String>,
    },
    /// Get one AI application by UUID.
    Get {
        /// Application UUID.
        id: String,
    },
}

#[derive(Subcommand)]
enum EvaluationsCmd {
    /// List configured evaluations (optionally scoped to one app).
    List {
        /// Scope to one application (pair with --subsystem).
        #[arg(long)]
        application: Option<String>,
        /// Scope to one subsystem (pair with --application).
        #[arg(long)]
        subsystem: Option<String>,
        /// Filter by evaluation type, as the API enum (e.g. PII, TOXICITY,
        /// PROMPT_INJECTION — the keys returned by `coverage`).
        #[arg(long = "evaluation-type")]
        evaluation_type: Option<String>,
        /// Maximum number of evaluations to return.
        #[arg(long)]
        page_size: Option<u32>,
        /// Number of evaluations to skip for pagination.
        #[arg(long)]
        page_offset: Option<u32>,
    },
    /// Get one configured evaluation by UUID.
    Get {
        /// Evaluation UUID.
        id: String,
    },
    /// Create (enable) an evaluation on an application from a JSON file [requires --yes].
    Create {
        /// Path to JSON file with the evaluation body. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a configured evaluation by UUID from a JSON file [requires --yes].
    Update {
        /// Path to JSON file with the partial update. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// Evaluation UUID.
        id: String,
    },
    /// Delete a configured evaluation by UUID [requires --yes].
    Delete {
        /// Evaluation UUID.
        id: String,
    },
}

#[derive(Subcommand)]
enum CustomEvaluationsCmd {
    /// List all custom evaluation policies.
    List,
    /// List custom evaluations linked to an application (by UUID).
    ListForApplication {
        /// Application UUID.
        application_id: String,
    },
    /// Create a custom evaluation policy from a JSON file [requires --yes].
    Create {
        /// Path to JSON file with the custom evaluation body. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a custom evaluation policy by UUID from a JSON file [requires --yes].
    Update {
        /// Path to JSON file with the partial update. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// Custom evaluation UUID.
        id: String,
    },
    /// Attach a custom evaluation (policy) to an application [requires --yes].
    Add {
        /// Custom evaluation UUID.
        evaluation_id: String,
        /// Application UUID.
        application_id: String,
    },
    /// Detach a custom evaluation (policy) from an application [requires --yes].
    Remove {
        /// Custom evaluation UUID.
        evaluation_id: String,
        /// Application UUID.
        application_id: String,
    },
}

#[derive(Subcommand)]
enum ModelPricingCmd {
    /// Get the team's custom per-model pricing overrides.
    Get,
    /// Set the team's per-model pricing overrides from a JSON file [requires --yes].
    Set {
        /// Path to JSON file with the model→price map. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
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
    /// Create an API key from a JSON definition file [requires --yes].
    Create {
        /// Path to JSON file with the API key definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update an API key from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated API key definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// API key ID.
        id: String,
    },
    /// Delete an API key [requires --yes].
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
    /// Bulk delete API keys by IDs [requires --yes].
    Delete {
        /// API key IDs to delete.
        #[arg(long, num_args = 1..)]
        ids: Vec<String>,
    },
    /// Set active/inactive status for API keys [requires --yes].
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
    /// Create a custom role from a JSON definition file [requires --yes].
    Create {
        /// Path to JSON file with the role definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a custom role from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated role definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// Role ID.
        id: String,
    },
    /// Delete a custom role [requires --yes].
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
    /// Create a scope from a JSON definition file [requires --yes].
    Create {
        /// Path to JSON file with the scope definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a scope from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated scope definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete a scope [requires --yes].
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
    /// Create user(s) from a JSON definition file [requires --yes].
    Create {
        /// Path to JSON file with the user definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update user(s) from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated user definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Set status for one or more users [requires --yes].
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
    /// Create a team group from a JSON definition file [requires --yes].
    Create {
        /// Path to JSON file with the group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update a team group from a JSON definition file [requires --yes].
    Update {
        /// Path to JSON file with the updated group definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
        /// Team group ID.
        id: String,
    },
    /// Delete a team group [requires --yes].
    Delete {
        /// Team group ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum IpAccessCmd {
    /// Get IP access settings.
    Get,
    /// Create IP access settings from a JSON file [requires --yes].
    Create {
        /// Path to JSON file with IP access settings. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update IP access settings from a JSON file [requires --yes].
    Update {
        /// Path to JSON file with updated IP access settings. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete IP access settings [requires --yes].
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
    /// Create an action from a JSON file [requires --yes].
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace an action from a JSON file [requires --yes].
    Update {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an action [requires --yes].
    Delete {
        /// Action ID.
        id: String,
    },
    /// Batch execute actions from a JSON file [requires --yes].
    Batch {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Reorder actions from a JSON file [requires --yes].
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
    /// Create metrics archive from a JSON file [requires --yes].
    Create {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Update metrics archive from a JSON file [requires --yes].
    Update {
        /// Path to JSON file. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Enable metrics archiving [requires --yes].
    Enable,
    /// Disable metrics archiving [requires --yes].
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
    /// Set logs archive target from a JSON file [requires --yes].
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
    /// Create an SLO from a JSON definition file [requires --yes].
    #[command(after_help = "\
Examples:
  cx slos create --from-file slo.json
  cat slo.json | cx slos create")]
    Create {
        /// Path to JSON file with the SLO definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Replace an SLO definition from a JSON file [requires --yes].
    #[command(after_help = "\
Examples:
  cx slos update --from-file slo.json")]
    Update {
        /// Path to JSON file with the updated SLO definition. Use '-' for stdin.
        #[arg(long, default_value = "-")]
        from_file: String,
    },
    /// Delete an SLO [requires --yes].
    Delete {
        /// SLO ID (UUID).
        id: String,
    },
}

#[derive(Subcommand)]
enum InfraCmd {
    /// Query infrastructure resources.
    #[command(after_help = "\
Examples:
  cx infra resources types
  cx infra resources list --category Hosts --type EC2_Instances --scope environment=prod
  cx infra resources health-history \"1001234:host_id=i-abc123\"
  cx infra resources raw-data \"1001234:host_id=i-abc123\"")]
    Resources {
        #[command(subcommand)]
        cmd: InfraResourcesCmd,
    },
}

#[derive(Subcommand)]
enum InfraResourcesCmd {
    /// List the available resource types (category/type pairs).
    Types,
    /// List resources of a given category and type.
    #[command(after_help = "\
Examples:
  cx infra resources list --category Hosts --type EC2_Instances
  cx infra resources list --category Hosts --type EC2_Instances --name-filter web
  cx infra resources list --category Hosts --type EC2_Instances --scope service=checkout --scope environment=prod
  cx infra resources list --category Hosts --type EC2_Instances --start-row 100 --end-row 200")]
    List {
        /// Resource category (discover with `cx infra resources types`).
        #[arg(long)]
        category: String,

        /// Resource type within the category (discover with `cx infra resources types`).
        #[arg(long)]
        r#type: String,

        /// Filter resources by name.
        #[arg(long)]
        name_filter: Option<String>,

        /// Scope filter as key=value; repeatable across different keys, at most
        /// once per key. Keys: service, environment, team. Multiple keys AND together.
        #[arg(long)]
        scope: Vec<String>,

        /// First row of the page window (0-based; default 0).
        #[arg(long)]
        start_row: Option<i64>,

        /// Row after the last one of the page window, exclusive (default:
        /// start-row + 100). The API rejects windows reaching past row 10,000.
        #[arg(long)]
        end_row: Option<i64>,
    },
    /// Show the daily health status history for a resource.
    #[command(after_help = "\
Examples:
  cx infra resources health-history \"1001234:host_id=i-abc123\"")]
    HealthHistory {
        /// Resource ID, exactly as returned by `cx infra resources list`.
        resource_id: String,
    },
    /// Fetch the raw resource document as JSON.
    #[command(after_help = "\
Examples:
  cx infra resources raw-data \"1001234:host_id=i-abc123\"
  cx infra resources raw-data \"1001234:host_id=i-abc123\" -o json")]
    RawData {
        /// Resource ID, exactly as returned by `cx infra resources list`.
        resource_id: String,
    },
}

#[derive(Subcommand)]
enum ServiceCatalogCmd {
    /// List the entity types this account has service-catalog data for.
    EntityTypes,
    /// Show the columns/labels schema for one entity type.
    #[command(after_help = "\
Examples:
  cx service-catalog schema service
  cx service-catalog schema k8s-pod")]
    Schema {
        /// Entity type: service, database, operation, database-operation, jvm,
        /// jvm-gc, k8s-pod, or transaction (also accepts the full
        /// ENTITY_TYPE_* proto name).
        entity_type: String,
    },
    /// List the known entities (e.g. service names) of one entity type.
    #[command(after_help = "\
Examples:
  cx service-catalog entities service")]
    Entities {
        /// Entity type (see `schema` for accepted values).
        entity_type: String,
    },
    /// Get column data (RED metrics, health, resource saturation, dependencies)
    /// across every entity of one type.
    #[command(after_help = "\
Examples:
  cx service-catalog data service --start now-1h --end now --column latency_p99 --column error_rate
  cx service-catalog data k8s-pod --start now-1h --end now --column cpu_usage --column oom_killed
  cx service-catalog data service --start now-1h --end now --column latency_p99 \\
    --filter environment=prod --aggregation table --sort-column latency_p99 --sort-order desc --limit 10
  cx service-catalog data service --start now-1h --end now --column latency_p99 --aggregation timeseries")]
    Data {
        /// Entity type (see `schema` for accepted values).
        entity_type: String,

        /// Start of the time range: `now`, `now-1h`, `now - 3d`, or ISO-8601.
        #[arg(long)]
        start: String,

        /// End of the time range: `now`, `now-1h`, `now - 3d`, or ISO-8601.
        #[arg(long)]
        end: String,

        /// Column id to fetch. Repeatable; at least one required. Discover
        /// valid ids with `cx service-catalog schema <entity-type>`.
        #[arg(long = "column", required = true)]
        columns: Vec<String>,

        /// Label to group rows by, on top of the entity identity. Repeatable.
        #[arg(long = "group-by")]
        group_by: Vec<String>,

        /// Filter as label=value1,value2; repeatable across different labels,
        /// at most once per label. Multiple labels combine with AND.
        #[arg(long = "filter")]
        filters: Vec<String>,

        /// Response shape: `table` (default) or `timeseries`.
        #[arg(long, default_value = "table")]
        aggregation: String,

        /// Max rows to return. `table` aggregation only.
        #[arg(long)]
        limit: Option<i32>,

        /// Column id to sort by. `table` aggregation only.
        #[arg(long)]
        sort_column: Option<String>,

        /// Sort direction: `asc` or `desc`. `table` aggregation only.
        #[arg(long)]
        sort_order: Option<String>,
    },
    /// Get column data for exactly one named entity (drilldown).
    #[command(after_help = "\
Examples:
  cx service-catalog entity-data service checkout --start now-1h --end now --column latency_p99")]
    EntityData {
        /// Entity type (see `schema` for accepted values).
        entity_type: String,

        /// Entity id, exactly as returned by `cx service-catalog entities`.
        entity_id: String,

        /// Start of the time range: `now`, `now-1h`, `now - 3d`, or ISO-8601.
        #[arg(long)]
        start: String,

        /// End of the time range: `now`, `now-1h`, `now - 3d`, or ISO-8601.
        #[arg(long)]
        end: String,

        /// Column id to fetch. Repeatable; at least one required. Discover
        /// valid ids with `cx service-catalog schema <entity-type>`.
        #[arg(long = "column", required = true)]
        columns: Vec<String>,

        /// Label to group rows by, on top of the entity identity. Repeatable.
        #[arg(long = "group-by")]
        group_by: Vec<String>,

        /// Filter as label=value1,value2; repeatable across different labels,
        /// at most once per label. Multiple labels combine with AND.
        #[arg(long = "filter")]
        filters: Vec<String>,

        /// Response shape: `table` (default) or `timeseries`.
        #[arg(long, default_value = "table")]
        aggregation: String,
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

    // Kick off the background version check.  The result is written to
    // ~/.cx/state.json and read back on the next command invocation.
    // For fast local commands the task may be cancelled before it finishes
    // writing (intentional race — same model as `gh`); API commands take long
    // enough that the fetch always completes in time.
    let _update_task = tokio::spawn(update_check::fetch_if_stale());

    // Check if this is a profiles command - use separate parser without global API flags.
    // Only works when `profiles` is the first arg (no global flags before it).
    if std::env::args().nth(1).as_deref() == Some("profiles") {
        let profile_matches = ProfilesCli::command().get_matches();
        let profiles_cli = ProfilesCli::from_arg_matches(&profile_matches)?;
        let ProfilesTopLevel::Profiles { cmd } = profiles_cli.command;
        let result = match cmd {
            ProfilesCmd::List => commands::profiles::run_list(),
            ProfilesCmd::Add {
                name,
                name_flag,
                url,
                region,
                api_key,
                oauth,
                force,
                set_default,
                disable_olly,
            } => {
                commands::profiles::run_add(commands::profiles::AddArgs {
                    name: name.or(name_flag),
                    url,
                    region,
                    api_key,
                    oauth,
                    force,
                    set_default,
                    disable_olly,
                    quick: false,
                })
                .await
            }
            ProfilesCmd::Refresh { name } => commands::profiles::run_refresh(name).await,
            ProfilesCmd::Delete { name, force } => commands::profiles::run_delete(name, force),
            ProfilesCmd::SetDefault { name } => commands::profiles::run_set_default(name),
        };
        update_check::maybe_print_notice(OutputFormat::Text);
        return result;
    }
    // When global flags precede `profiles` (e.g. `cx --read-only profiles list`),
    // the early check above misses it. The main Cli parser handles it below.

    let mut cmd = Cli::command();
    if banner::should_show() {
        cmd = cmd.before_help(banner::render()).help_template(
            "{before-help}\n{usage-heading} {usage}{after-help}\n\n\x1b[1m\x1b[4mGlobal Options:\x1b[0m\n{options}",
        );
    }

    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches)?;
    let yes = cli.yes;
    // Load global config early for read-only / risky / olly gating.
    let global_cfg_early = config::load_config().unwrap_or_default();

    let read_only =
        cli.read_only || safety::env_is_truthy("CX_READ_ONLY") || global_cfg_early.read_only;
    let no_console_link = cli.no_console_link
        || safety::env_is_truthy("CX_NO_CONSOLE_LINK")
        || global_cfg_early.no_console_link;
    if read_only {
        let top = safety::get_top_level_subcommand_name(&matches);
        let is_local = matches!(
            top.as_deref(),
            Some("profiles")
                | Some("cleanup")
                | Some("completions")
                | Some("docs")
                | Some("skills")
                | Some("init")
        );
        if !is_local {
            if let Some(leaf) = safety::get_leaf_subcommand_name(&matches) {
                safety::enforce_read_only(&leaf)?;
            }
        }
    }

    // Risky command gating (iam, archive write operations).
    if cli.command.is_risky() && !global_cfg_early.allow_risky_commands {
        if let Some(leaf) = safety::get_leaf_subcommand_name(&matches) {
            if safety::is_write_verb(&leaf) {
                bail!(
                    "Risky write operation blocked by global configuration.\n\
                     Set allow_risky_commands = true in ~/.cx/config.toml to enable."
                );
            }
        }
    }

    // Olly gating (olly ask).
    if cli.command.is_olly() && !global_cfg_early.olly_enabled {
        bail!(
            "The Olly AI assistant is currently disabled in your global configuration.\n\
             To enable it, set olly_enabled = true in ~/.cx/config.toml."
        );
    }

    // Profiles command is usually handled by the early ProfilesCli parser above,
    // but when global flags precede `profiles` (e.g. `cx --read-only profiles list`),
    // it falls through to here.
    if let Commands::Profiles { cmd } = cli.command {
        let result = match cmd {
            ProfilesCmd::List => commands::profiles::run_list(),
            ProfilesCmd::Add {
                name,
                name_flag,
                url,
                region,
                api_key,
                oauth,
                force,
                set_default,
                disable_olly,
            } => {
                commands::profiles::run_add(commands::profiles::AddArgs {
                    name: name.or(name_flag),
                    url,
                    region,
                    api_key,
                    oauth,
                    force,
                    set_default,
                    disable_olly,
                    quick: false,
                })
                .await
            }
            ProfilesCmd::Refresh { name } => commands::profiles::run_refresh(name).await,
            ProfilesCmd::Delete { name, force } => commands::profiles::run_delete(name, force),
            ProfilesCmd::SetDefault { name } => commands::profiles::run_set_default(name),
        };
        update_check::maybe_print_notice(OutputFormat::Text);
        return result;
    }

    // Docs commands fetch public documentation — no API credentials.
    if let Commands::Docs { ref cmd } = cli.command {
        let global_config = config::load_config().unwrap_or_default();
        let output = cli.output.unwrap_or(global_config.default_output_format);
        let result = match cmd {
            DocsCmd::Search { query, limit } => {
                commands::docs::run_search(query, *limit, output).await
            }
            DocsCmd::Fetch { suffix } => commands::docs::run_fetch(suffix, output).await,
        };
        update_check::maybe_print_notice(output);
        return result;
    }

    // Cleanup command doesn't need API credentials.
    if let Commands::Cleanup = cli.command {
        let result = commands::cleanup::run();
        update_check::maybe_print_notice(OutputFormat::Text);
        return result;
    }

    // Init chains profile setup + skills install locally - no API credentials
    // up front (the profile step acquires them). Handled before credential
    // resolution, like profiles/skills.
    if let Commands::Init {
        url,
        oauth,
        no_skills,
        global_skills,
        local_skills,
        agents,
        install_completions,
    } = cli.command
    {
        let scope = if global_skills {
            Some(commands::skills::SkillsScope::Global)
        } else if local_skills {
            Some(commands::skills::SkillsScope::Local)
        } else {
            None
        };
        let result = commands::init::run_init(commands::init::InitArgs {
            url,
            region: cli.region,
            api_key: cli.api_key,
            oauth,
            install_skills: !no_skills,
            agents,
            scope,
            install_completions,
        })
        .await;
        update_check::maybe_print_notice(OutputFormat::Text);
        return result;
    }

    // Skills install shells out to npx locally - no API credentials.
    if let Commands::Skills { cmd } = cli.command {
        let SkillsCmd::Install {
            global,
            local,
            agents,
            interactive,
        } = cmd;
        let result = if interactive {
            commands::skills::run_advanced_install()
        } else {
            let scope = if global {
                Some(commands::skills::SkillsScope::Global)
            } else if local {
                Some(commands::skills::SkillsScope::Local)
            } else {
                None
            };
            // The explicit command always (re)installs to update, so the
            // outcome is uninteresting here — only init branches on it.
            commands::skills::run_install(commands::skills::InstallOptions {
                scope,
                agents,
                skip_if_installed: false,
            })
            .map(|_| ())
        };
        update_check::maybe_print_notice(OutputFormat::Text);
        return result;
    }

    // Schema command doesn't need API credentials - outputs command tree as JSON.
    // The _meta.update block is already embedded in the JSON output for toon mode;
    // the stderr notice covers TTY human users (or plain text for toon mode).
    if let Commands::Schema = cli.command {
        let result = commands::schema::run(Cli::command());
        let output = cli.output.unwrap_or(OutputFormat::Text);
        update_check::maybe_print_notice(output);
        return result;
    }

    // Completions commands don't need API credentials.
    if let Commands::Completions { cmd } = cli.command {
        let result = match cmd {
            CompletionsCmd::Generate { shell } => {
                commands::completions::run_generate(shell, &mut Cli::command())
            }
            CompletionsCmd::Install { shell, path } => {
                commands::completions::run_install(shell, path)
            }
            CompletionsCmd::Refresh => commands::completions::run_refresh(Cli::command),
        };
        update_check::maybe_print_notice(OutputFormat::Text);
        return result;
    }

    // Dataprime list/show don't need API credentials - handle them early.
    if let Commands::Dataprime { ref cmd } = cli.command {
        if matches!(cmd, DataprimeCmd::List { .. } | DataprimeCmd::Show { .. }) {
            let global_config = config::load_config().unwrap_or_default();
            let output = cli.output.unwrap_or(global_config.default_output_format);
            let result = match cmd {
                DataprimeCmd::List { filter, name } => {
                    commands::dataprime::run_list(*filter, name.as_deref(), output)
                }
                DataprimeCmd::Show { name } => commands::dataprime::run_help(name, output),
                // Query needs credentials - handled in the main match below.
                DataprimeCmd::Query { .. } => unreachable!(),
            };
            update_check::maybe_print_notice(output);
            return result;
        }
        // DataprimeCmd::Query needs credentials - fall through.
    }

    // When the user names a profile explicitly via --profile but did NOT pass
    // --api-key / --region on the command line, suppress the corresponding
    // env-var value (CX_API_KEY / CX_REGION) so the profile's own stored
    // credentials and region are used instead.
    let profile_from_cli = matches.value_source("profile") == Some(ValueSource::CommandLine);
    let api_key_from_cli = matches.value_source("api_key") == Some(ValueSource::CommandLine);
    let region_from_cli = matches.value_source("region") == Some(ValueSource::CommandLine);
    let effective_api_key: Option<&str> = if profile_from_cli && !api_key_from_cli {
        None
    } else {
        cli.api_key.as_deref()
    };
    let effective_region: Option<&str> = if profile_from_cli && !region_from_cli {
        None
    } else {
        cli.region.as_deref()
    };

    // Reject --api-key / --region when more than one --profile is supplied,
    // because it would be ambiguous which profile the override targets.
    if cli.profile.len() > 1 && (effective_api_key.is_some() || effective_region.is_some()) {
        bail!(
            "Cannot combine multiple --profile values with --api-key or --region overrides.\n\
             Store per-profile credentials with `cx profiles add <name>`."
        );
    }

    // Load global config for defaults (non-fatal - fall back to defaults).
    let global_config = config::load_config().unwrap_or_default();
    let output = cli
        .output
        .or_else(|| config::first_profile_output_format(&cli.profile))
        .unwrap_or(global_config.default_output_format);
    let max_direct = global_config.max_dataprime_direct_output_size;
    let temp_dir = global_config.temp_dir.clone();

    // Resolve one or more profiles into execution targets.
    let configs = match config::resolve_all(&cli.profile, effective_api_key, effective_region).await
    {
        Ok(configs) => configs,
        Err(error) => {
            // First-run guidance: when nothing is configured at all (no profile
            // on disk and no env-only credentials), don't dump the underlying
            // config-resolution error. Point the user at the single guided entry
            // point instead. The onboarding commands that *fix* this state
            // (`cx init`, `cx profiles add`, `cx skills`) are handled earlier and
            // never reach here, so they can't be short-circuited by this branch.
            if config::list_profile_names()
                .map(|names| names.is_empty())
                .unwrap_or(false)
            {
                eprintln!("No Coralogix profile is configured.");
                eprintln!("Run `cx init` to set up a profile and get started.");
                // Exit here instead of returning the error: propagating it
                // would dump the anyhow config-resolution chain (with a second,
                // contradicting `cx profiles add` instruction) after the
                // guidance. The two lines above are the entire first-run story.
                std::process::exit(1);
            }
            eprintln!("Configuration error: {error}");
            eprintln!("Run `cx profiles add` to set up credentials.");
            return Err(error);
        }
    };

    let targets = build_targets(configs, no_console_link)?;
    let agent_mode = safety::is_agent_mode();

    // Wrap the dispatch in an async block so we can capture its Result and
    // always run the update notice afterwards (even on error).
    let cmd_result = async {
        match cli.command {
            Commands::Profiles { .. } => unreachable!("handled by ProfilesCli above"),
            Commands::Init { .. } => unreachable!("handled above"),
            Commands::Cleanup => unreachable!("handled above"),
            Commands::Skills { .. } => unreachable!("handled above"),
            Commands::Schema => unreachable!("handled above"),
            Commands::Completions { .. } => unreachable!("handled above"),
            Commands::Docs { .. } => unreachable!("handled above"),

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
                    commands::metrics::run_query_range(
                        &targets, &expr, &start, &end, &step, output,
                    )
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
                    confirm_destructive("Create a new dashboard?", yes, agent_mode)?;
                    commands::dashboards::run_create(
                        &targets,
                        &from_file,
                        folder.as_deref(),
                        output,
                    )
                    .await?;
                }
                DashboardsCmd::Replace { from_file } => {
                    commands::dashboards::run_replace(
                        &targets, &from_file, output, yes, agent_mode,
                    )
                    .await?;
                }
                DashboardsCmd::Check {
                    from_file,
                    dashboard_id,
                } => {
                    commands::dashboards::run_check(
                        &targets,
                        from_file.as_deref(),
                        dashboard_id.as_deref(),
                        output,
                    )
                    .await?;
                }
                DashboardsCmd::Delete { dashboard_id } => {
                    confirm_destructive(
                        &format!("Delete dashboard '{dashboard_id}'?"),
                        yes,
                        agent_mode,
                    )?;
                    commands::dashboards::run_delete(&targets, &dashboard_id).await?;
                }
                DashboardsCmd::Search { description, limit } => {
                    commands::dashboards::run_semantic_search(
                        &targets,
                        &description,
                        limit,
                        output,
                    )
                    .await?;
                }
                DashboardsCmd::QuerySearch {
                    field,
                    description,
                    limit,
                } => match (field.as_deref(), description.as_deref()) {
                    (Some(f), _) => {
                        commands::dashboards::run_queries_by_field(&targets, f, limit, output)
                            .await?;
                    }
                    (_, Some(d)) => {
                        commands::dashboards::run_search(&targets, d, limit, output).await?;
                    }
                    (None, None) => {
                        bail!("specify --field or --description");
                    }
                },
                DashboardsCmd::Folders { cmd } => match cmd {
                    FoldersCmd::List => {
                        commands::dashboards::run_folders_list(&targets, output).await?;
                    }
                    FoldersCmd::Create { name, parent_id } => {
                        confirm_destructive("Create a new dashboard folder?", yes, agent_mode)?;
                        commands::dashboards::run_folders_create(
                            &targets,
                            &name,
                            parent_id.as_deref(),
                            output,
                        )
                        .await?;
                    }
                    FoldersCmd::Delete { id } => {
                        confirm_destructive(
                            &format!("Delete dashboard folder '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::dashboards::run_folders_delete(&targets, &id).await?;
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
                    confirm_destructive("Create a new alert?", yes, agent_mode)?;
                    commands::alerts::run_create(&targets, &from_file, output).await?;
                }
                AlertsCmd::Delete { alert_id } => {
                    confirm_destructive(&format!("Delete alert '{alert_id}'?"), yes, agent_mode)?;
                    commands::alerts::run_delete(&targets, &alert_id).await?;
                }
                AlertsCmd::Enable { alert_id } => {
                    confirm_destructive(&format!("Enable alert '{alert_id}'?"), yes, agent_mode)?;
                    commands::alerts::run_enable(&targets, &alert_id).await?;
                }
                AlertsCmd::Disable { alert_id } => {
                    confirm_destructive(&format!("Disable alert '{alert_id}'?"), yes, agent_mode)?;
                    commands::alerts::run_disable(&targets, &alert_id).await?;
                }
                AlertsCmd::Events {
                    alert_version_id,
                    start,
                    end,
                } => {
                    commands::alerts::run_events(
                        &targets,
                        &alert_version_id,
                        start.as_deref(),
                        end.as_deref(),
                        output,
                    )
                    .await?;
                }
                AlertsCmd::EventStats => {
                    commands::alerts::run_event_stats(&targets, output).await?;
                }
                AlertsCmd::SuppressionRules { cmd } => match cmd {
                    SuppressionRulesCmd::List => {
                        commands::suppression_rules::run_list(&targets, output).await?;
                    }
                    SuppressionRulesCmd::Get { id } => {
                        commands::suppression_rules::run_get(&targets, &id, output).await?;
                    }
                    SuppressionRulesCmd::Create { from_file } => {
                        confirm_destructive("Create a new suppression rule?", yes, agent_mode)?;
                        commands::suppression_rules::run_create(&targets, &from_file, output)
                            .await?;
                    }
                    SuppressionRulesCmd::Update { from_file } => {
                        confirm_destructive("Update suppression rule?", yes, agent_mode)?;
                        commands::suppression_rules::run_update(&targets, &from_file, output)
                            .await?;
                    }
                    SuppressionRulesCmd::Delete { id } => {
                        confirm_destructive(
                            &format!("Delete suppression rule '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::suppression_rules::run_delete(&targets, &id).await?;
                    }
                },
            },

            Commands::Cases { cmd } => match cmd {
                CasesCmd::Get { id } => {
                    commands::cases::run_get(&targets, &id, output).await?;
                }
                CasesCmd::Update {
                    id,
                    title,
                    resolution_reason,
                } => {
                    commands::cases::run_update(
                        &targets,
                        &id,
                        title.as_deref(),
                        resolution_reason.as_deref(),
                        output,
                    )
                    .await?;
                }
                CasesCmd::Comment { id, text } => {
                    commands::cases::run_comment(&targets, &id, &text, output).await?;
                }
                CasesCmd::Assign { id, user } => {
                    commands::cases::run_assign(&targets, &id, &user, output).await?;
                }
                CasesCmd::Unassign { id } => {
                    commands::cases::run_unassign(&targets, &id, output).await?;
                }
                CasesCmd::Acknowledge { id } => {
                    commands::cases::run_acknowledge(&targets, &id, output).await?;
                }
                CasesCmd::Unacknowledge { id } => {
                    commands::cases::run_unacknowledge(&targets, &id, output).await?;
                }
                CasesCmd::Resolve {
                    id,
                    reason,
                    no_reason,
                } => {
                    let reason = match reason {
                        Some(r) => Some(r),
                        // `--no-reason` is the explicit opt-out; resolve with no reason.
                        None if no_reason => None,
                        // Otherwise a reason is required. In an interactive terminal we
                        // prompt for it; in non-interactive/agent/--yes contexts we refuse
                        // rather than silently resolving with an empty audit trail.
                        None => match safety::prompt_optional_text(
                            "Resolution reason:",
                            Some(
                                "Share the root cause, what fixed it, and any follow-up. \
                             Visible to all teammates in the case timeline.",
                            ),
                            yes,
                            agent_mode,
                        )? {
                            Some(r) => Some(r),
                            None => anyhow::bail!(
                                "a resolution reason is required: pass --reason \"<text>\", \
                                 or --no-reason to resolve without one"
                            ),
                        },
                    };
                    confirm_destructive(
                        &format!(
                            "Resolve case '{id}'? Resolution is irreversible — \
                         the case cannot be reopened, only closed."
                        ),
                        yes,
                        agent_mode,
                    )?;
                    commands::cases::run_resolve(&targets, &id, reason.as_deref(), output).await?;
                }
                CasesCmd::Close { id } => {
                    commands::cases::run_close(&targets, &id, output).await?;
                }
                CasesCmd::SetPriority { id, priority } => {
                    commands::cases::run_set_priority(&targets, &id, &priority, output).await?;
                }
                CasesCmd::ClearPriority { id } => {
                    commands::cases::run_clear_priority(&targets, &id, output).await?;
                }
                CasesCmd::Events { cmd } => match cmd {
                    CasesEventsCmd::List { case_id } => {
                        commands::cases::run_events_list(&targets, &case_id, output).await?;
                    }
                    CasesEventsCmd::Get { event_id } => {
                        commands::cases::run_event_get(&targets, &event_id, output).await?;
                    }
                },
                CasesCmd::Notifications { case_ids } => {
                    commands::cases::run_notifications(&targets, &case_ids, output).await?;
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
                        confirm_destructive("Create a new connector?", yes, agent_mode)?;
                        commands::connectors::run_create(&targets, &from_file, output).await?;
                    }
                    ConnectorsCmd::Update { from_file } => {
                        confirm_destructive("Update connector?", yes, agent_mode)?;
                        commands::connectors::run_update(&targets, &from_file, output).await?;
                    }
                    ConnectorsCmd::Delete { id } => {
                        confirm_destructive(&format!("Delete connector '{id}'?"), yes, agent_mode)?;
                        commands::connectors::run_delete(&targets, &id).await?;
                    }
                    ConnectorsCmd::Types => {
                        commands::connectors::run_types(&targets, output).await?;
                    }
                    ConnectorsCmd::EntityTypes => {
                        commands::connectors::run_entity_types(&targets, output).await?;
                    }
                    ConnectorsCmd::EntitySubtypes { r#type } => {
                        commands::connectors::run_entity_subtypes(&targets, &r#type, output)
                            .await?;
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
                        confirm_destructive("Create a new router?", yes, agent_mode)?;
                        commands::routers::run_create(&targets, &from_file, output).await?;
                    }
                    RoutersCmd::Update { from_file } => {
                        confirm_destructive("Update router?", yes, agent_mode)?;
                        commands::routers::run_update(&targets, &from_file, output).await?;
                    }
                    RoutersCmd::Delete { id } => {
                        confirm_destructive(&format!("Delete router '{id}'?"), yes, agent_mode)?;
                        commands::routers::run_delete(&targets, &id).await?;
                    }
                    RoutersCmd::ValidateMatcher { from_file } => {
                        commands::routers::run_validate_matcher(&targets, &from_file, output)
                            .await?;
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
                        confirm_destructive("Create a new preset?", yes, agent_mode)?;
                        commands::presets::run_create(&targets, &from_file, output).await?;
                    }
                    PresetsCmd::Update { from_file } => {
                        confirm_destructive("Update preset?", yes, agent_mode)?;
                        commands::presets::run_update(&targets, &from_file, output).await?;
                    }
                    PresetsCmd::Delete { id } => {
                        confirm_destructive(&format!("Delete preset '{id}'?"), yes, agent_mode)?;
                        commands::presets::run_delete(&targets, &id).await?;
                    }
                    PresetsCmd::SetDefault { id } => {
                        confirm_destructive(
                            &format!("Set preset '{id}' as default?"),
                            yes,
                            agent_mode,
                        )?;
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
                        commands::notification_testing::run_test_preset(
                            &targets, &from_file, output,
                        )
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
                DataUsageCmd::LogsCount {
                    start,
                    end,
                    resolution,
                    subsystem_aggregation,
                    application_aggregation,
                    params,
                } => {
                    commands::data_usage::run_logs_count(
                        &targets,
                        commands::data_usage::CountCommandOptions {
                            start: start.as_deref(),
                            end: end.as_deref(),
                            resolution: resolution.as_deref(),
                            subsystem_aggregation,
                            application_aggregation,
                            extra_params: &params,
                            output,
                        },
                    )
                    .await?;
                }
                DataUsageCmd::SpansCount {
                    start,
                    end,
                    resolution,
                    subsystem_aggregation,
                    application_aggregation,
                    params,
                } => {
                    commands::data_usage::run_spans_count(
                        &targets,
                        commands::data_usage::CountCommandOptions {
                            start: start.as_deref(),
                            end: end.as_deref(),
                            resolution: resolution.as_deref(),
                            subsystem_aggregation,
                            application_aggregation,
                            extra_params: &params,
                            output,
                        },
                    )
                    .await?;
                }
                DataUsageCmd::Capabilities => {
                    commands::data_usage::run_capabilities(&targets, output).await?;
                }
                DataUsageCmd::Query { from_file, query } => {
                    commands::data_usage::run_query(
                        &targets,
                        from_file.as_deref(),
                        query.as_deref(),
                        output,
                    )
                    .await?;
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
                    confirm_destructive("Create a new TCO policy?", yes, agent_mode)?;
                    commands::tco_policies::run_create(&targets, &from_file, output).await?;
                }
                TcoPoliciesCmd::Update { from_file } => {
                    confirm_destructive("Update TCO policy?", yes, agent_mode)?;
                    commands::tco_policies::run_update(&targets, &from_file, output).await?;
                }
                TcoPoliciesCmd::Delete { id } => {
                    confirm_destructive(&format!("Delete TCO policy '{id}'?"), yes, agent_mode)?;
                    commands::tco_policies::run_delete(&targets, &id).await?;
                }
                TcoPoliciesCmd::Reorder { from_file } => {
                    confirm_destructive("Reorder TCO policies?", yes, agent_mode)?;
                    commands::tco_policies::run_reorder(&targets, &from_file, output).await?;
                }
                TcoPoliciesCmd::Test { from_file } => {
                    commands::tco_policies::run_test(&targets, &from_file, output).await?;
                }
                TcoPoliciesCmd::Settings => {
                    commands::tco_policies::run_settings(&targets, output).await?;
                }
                TcoPoliciesCmd::SettingsUpdate { from_file } => {
                    confirm_destructive("Update TCO settings?", yes, agent_mode)?;
                    commands::tco_policies::run_settings_update(&targets, &from_file, output)
                        .await?;
                }
            },

            Commands::Retentions { cmd } => match cmd {
                RetentionsCmd::List => {
                    commands::retentions::run_list(&targets, output).await?;
                }
                RetentionsCmd::Update { from_file } => {
                    confirm_destructive("Update retention settings?", yes, agent_mode)?;
                    commands::retentions::run_update(&targets, &from_file, output).await?;
                }
                RetentionsCmd::Activate => {
                    confirm_destructive("Activate retention settings?", yes, agent_mode)?;
                    commands::retentions::run_activate(&targets).await?;
                }
                RetentionsCmd::Status => {
                    commands::retentions::run_status(&targets, output).await?;
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
                    confirm_destructive("Create a new E2M definition?", yes, agent_mode)?;
                    commands::e2m::run_create(&targets, &from_file, output).await?;
                }
                E2mCmd::Update { from_file } => {
                    confirm_destructive("Update E2M definition?", yes, agent_mode)?;
                    commands::e2m::run_update(&targets, &from_file, output).await?;
                }
                E2mCmd::Delete { id } => {
                    confirm_destructive(
                        &format!("Delete E2M definition '{id}'?"),
                        yes,
                        agent_mode,
                    )?;
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
                    confirm_destructive("Create a new recording rule group?", yes, agent_mode)?;
                    commands::recording_rules::run_create(&targets, &from_file, output).await?;
                }
                RecordingRulesCmd::Update { from_file, id } => {
                    confirm_destructive(
                        &format!("Update recording rule group '{id}'?"),
                        yes,
                        agent_mode,
                    )?;
                    commands::recording_rules::run_update(&targets, &id, &from_file, output)
                        .await?;
                }
                RecordingRulesCmd::Delete { id } => {
                    confirm_destructive(
                        &format!("Delete recording rule group '{id}'?"),
                        yes,
                        agent_mode,
                    )?;
                    commands::recording_rules::run_delete(&targets, &id).await?;
                }
            },

            Commands::ParsingRules { cmd } => match cmd {
                ParsingRulesCmd::List => {
                    commands::parsing_rules::run_list(&targets, output).await?;
                }
                ParsingRulesCmd::Get { id } => {
                    commands::parsing_rules::run_get(&targets, &id, output).await?;
                }
                ParsingRulesCmd::Create { from_file } => {
                    confirm_destructive("Create a new parsing rule?", yes, agent_mode)?;
                    commands::parsing_rules::run_create(&targets, &from_file, output).await?;
                }
                ParsingRulesCmd::Update { from_file, id } => {
                    confirm_destructive(&format!("Update parsing rule '{id}'?"), yes, agent_mode)?;
                    commands::parsing_rules::run_update(&targets, &id, &from_file, output).await?;
                }
                ParsingRulesCmd::Delete { id } => {
                    confirm_destructive(&format!("Delete parsing rule '{id}'?"), yes, agent_mode)?;
                    commands::parsing_rules::run_delete(&targets, &id).await?;
                }
                ParsingRulesCmd::BulkDelete { ids } => {
                    confirm_destructive("Bulk delete parsing rules?", yes, agent_mode)?;
                    commands::parsing_rules::run_bulk_delete(&targets, &ids).await?;
                }
                ParsingRulesCmd::UsageLimits => {
                    commands::parsing_rules::run_usage_limits(&targets, output).await?;
                }
            },

            Commands::Enrichments { cmd } => match cmd {
                EnrichmentsCmd::List => {
                    commands::enrichments::run_list(&targets, output).await?;
                }
                EnrichmentsCmd::Add { from_file } => {
                    confirm_destructive("Add enrichment rules?", yes, agent_mode)?;
                    commands::enrichments::run_add(&targets, &from_file, output).await?;
                }
                EnrichmentsCmd::Remove { from_file } => {
                    confirm_destructive("Remove enrichment rules?", yes, agent_mode)?;
                    commands::enrichments::run_remove(&targets, &from_file, output).await?;
                }
                EnrichmentsCmd::Overwrite { from_file } => {
                    confirm_destructive("Overwrite enrichment rules?", yes, agent_mode)?;
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
                        confirm_destructive("Create a new custom enrichment?", yes, agent_mode)?;
                        commands::custom_enrichments::run_create(&targets, &from_file, output)
                            .await?;
                    }
                    CustomEnrichmentsCmd::Update { from_file } => {
                        confirm_destructive("Update custom enrichment?", yes, agent_mode)?;
                        commands::custom_enrichments::run_update(&targets, &from_file, output)
                            .await?;
                    }
                    CustomEnrichmentsCmd::Delete { id } => {
                        confirm_destructive(
                            &format!("Delete custom enrichment '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::custom_enrichments::run_delete(&targets, &id).await?;
                    }
                    CustomEnrichmentsCmd::Search { id, query } => {
                        commands::custom_enrichments::run_search(&targets, &id, &query, output)
                            .await?;
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
                    confirm_destructive("Create a new integration?", yes, agent_mode)?;
                    commands::integrations::run_create(&targets, &from_file, output).await?;
                }
                IntegrationsCmd::Update { id, from_file } => {
                    confirm_destructive(&format!("Update integration '{id}'?"), yes, agent_mode)?;
                    commands::integrations::run_update(&targets, &id, &from_file, output).await?;
                }
                IntegrationsCmd::Delete { id } => {
                    confirm_destructive(&format!("Delete integration '{id}'?"), yes, agent_mode)?;
                    commands::integrations::run_delete(&targets, &id).await?;
                }
                IntegrationsCmd::Test { from_file } => {
                    confirm_destructive("Test integration?", yes, agent_mode)?;
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
                        confirm_destructive("Deploy extension?", yes, agent_mode)?;
                        commands::extensions::run_deploy(&targets, &from_file, output).await?;
                    }
                    ExtensionsCmd::Update { from_file } => {
                        confirm_destructive("Update extension?", yes, agent_mode)?;
                        commands::extensions::run_update(&targets, &from_file, output).await?;
                    }
                    ExtensionsCmd::Undeploy { from_file } => {
                        confirm_destructive("Undeploy extension?", yes, agent_mode)?;
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
                        confirm_destructive(
                            "Create contextual data integration?",
                            yes,
                            agent_mode,
                        )?;
                        commands::contextual_data::run_create(&targets, &from_file, output).await?;
                    }
                    ContextualDataCmd::Update { id, from_file } => {
                        confirm_destructive(
                            &format!("Update contextual data '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::contextual_data::run_update(&targets, &id, &from_file, output)
                            .await?;
                    }
                    ContextualDataCmd::Delete { id } => {
                        confirm_destructive(
                            &format!("Delete contextual data '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
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
                    confirm_destructive("Create a new webhook?", yes, agent_mode)?;
                    commands::webhooks::run_create(&targets, &from_file, output).await?;
                }
                WebhooksCmd::Update { id, from_file } => {
                    confirm_destructive(&format!("Update webhook '{id}'?"), yes, agent_mode)?;
                    commands::webhooks::run_update(&targets, &id, &from_file, output).await?;
                }
                WebhooksCmd::Delete { id } => {
                    confirm_destructive(&format!("Delete webhook '{id}'?"), yes, agent_mode)?;
                    commands::webhooks::run_delete(&targets, &id).await?;
                }
                WebhooksCmd::Test { id } => {
                    confirm_destructive(&format!("Test webhook '{id}'?"), yes, agent_mode)?;
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
                        confirm_destructive("Create a new webhook action?", yes, agent_mode)?;
                        commands::actions::run_create(&targets, &from_file, output).await?;
                    }
                    ActionsCmd::Update { from_file } => {
                        confirm_destructive("Update webhook action?", yes, agent_mode)?;
                        commands::actions::run_update(&targets, &from_file, output).await?;
                    }
                    ActionsCmd::Delete { id } => {
                        confirm_destructive(
                            &format!("Delete webhook action '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::actions::run_delete(&targets, &id).await?;
                    }
                    ActionsCmd::Batch { from_file } => {
                        confirm_destructive("Batch execute webhook actions?", yes, agent_mode)?;
                        commands::actions::run_batch(&targets, &from_file, output).await?;
                    }
                    ActionsCmd::Reorder { from_file } => {
                        confirm_destructive("Reorder webhook actions?", yes, agent_mode)?;
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
                    confirm_destructive("Create a new view?", yes, agent_mode)?;
                    commands::views::run_create(&targets, &from_file, output).await?;
                }
                ViewsCmd::Update { id, from_file } => {
                    confirm_destructive(&format!("Update view '{id}'?"), yes, agent_mode)?;
                    commands::views::run_update(&targets, &id, &from_file, output).await?;
                }
                ViewsCmd::Delete { id } => {
                    confirm_destructive(&format!("Delete view '{id}'?"), yes, agent_mode)?;
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
                        confirm_destructive("Create a new view folder?", yes, agent_mode)?;
                        commands::views::run_folders_create(&targets, &from_file, output).await?;
                    }
                    ViewFoldersCmd::Update { id, from_file } => {
                        confirm_destructive(
                            &format!("Update view folder '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::views::run_folders_update(&targets, &id, &from_file, output)
                            .await?;
                    }
                    ViewFoldersCmd::Delete { id } => {
                        confirm_destructive(
                            &format!("Delete view folder '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::views::run_folders_delete(&targets, &id).await?;
                    }
                },
            },

            Commands::AiCenter { cmd } => match cmd {
                AiCenterCmd::Applications { cmd } => match cmd {
                    ApplicationsCmd::List {
                        page_size,
                        page_offset,
                        evaluation_type,
                    } => {
                        commands::ai_center::run_applications_list(
                            &targets,
                            page_size,
                            page_offset,
                            &evaluation_type,
                            output,
                        )
                        .await?;
                    }
                    ApplicationsCmd::Get { id } => {
                        commands::ai_center::run_applications_get(&targets, &id, output).await?;
                    }
                },
                AiCenterCmd::Evaluations { cmd } => match cmd {
                    EvaluationsCmd::List {
                        application,
                        subsystem,
                        evaluation_type,
                        page_size,
                        page_offset,
                    } => {
                        commands::ai_center::run_evaluations_list(
                            &targets,
                            application.as_deref(),
                            subsystem.as_deref(),
                            evaluation_type.as_deref(),
                            page_size,
                            page_offset,
                            output,
                        )
                        .await?;
                    }
                    EvaluationsCmd::Get { id } => {
                        commands::ai_center::run_evaluations_get(&targets, &id, output).await?;
                    }
                    EvaluationsCmd::Create { from_file } => {
                        confirm_destructive("Create a new AI evaluation?", yes, agent_mode)?;
                        commands::ai_center::run_evaluations_create(&targets, &from_file, output)
                            .await?;
                    }
                    EvaluationsCmd::Update { from_file, id } => {
                        confirm_destructive(
                            &format!("Update AI evaluation '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::ai_center::run_evaluations_update(
                            &targets, &id, &from_file, output,
                        )
                        .await?;
                    }
                    EvaluationsCmd::Delete { id } => {
                        confirm_destructive(
                            &format!("Delete AI evaluation '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::ai_center::run_evaluations_delete(&targets, &id, output).await?;
                    }
                },
                AiCenterCmd::CustomEvaluations { cmd } => match cmd {
                    CustomEvaluationsCmd::List => {
                        commands::ai_center::run_custom_evaluations_list(&targets, output).await?;
                    }
                    CustomEvaluationsCmd::ListForApplication { application_id } => {
                        commands::ai_center::run_custom_evaluations_for_application(
                            &targets,
                            &application_id,
                            output,
                        )
                        .await?;
                    }
                    CustomEvaluationsCmd::Create { from_file } => {
                        confirm_destructive("Create a new custom evaluation?", yes, agent_mode)?;
                        commands::ai_center::run_custom_evaluations_create(
                            &targets, &from_file, output,
                        )
                        .await?;
                    }
                    CustomEvaluationsCmd::Update { from_file, id } => {
                        confirm_destructive(
                            &format!("Update custom evaluation '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::ai_center::run_custom_evaluations_update(
                            &targets, &id, &from_file, output,
                        )
                        .await?;
                    }
                    CustomEvaluationsCmd::Add {
                        evaluation_id,
                        application_id,
                    } => {
                        confirm_destructive(
                            &format!(
                                "Attach policy '{evaluation_id}' to application '{application_id}'?"
                            ),
                            yes,
                            agent_mode,
                        )?;
                        commands::ai_center::run_add_policy(
                            &targets,
                            &evaluation_id,
                            &application_id,
                            output,
                        )
                        .await?;
                    }
                    CustomEvaluationsCmd::Remove {
                        evaluation_id,
                        application_id,
                    } => {
                        confirm_destructive(
                            &format!(
                                "Detach policy '{evaluation_id}' from application '{application_id}'?"
                            ),
                            yes,
                            agent_mode,
                        )?;
                        commands::ai_center::run_remove_policy(
                            &targets,
                            &evaluation_id,
                            &application_id,
                            output,
                        )
                        .await?;
                    }
                },
                AiCenterCmd::Coverage => {
                    commands::ai_center::run_coverage(&targets, output).await?;
                }
                AiCenterCmd::ModelPricing { cmd } => match cmd {
                    ModelPricingCmd::Get => {
                        commands::ai_center::run_model_pricing_get(&targets, output).await?;
                    }
                    ModelPricingCmd::Set { from_file } => {
                        confirm_destructive("Set team model pricing?", yes, agent_mode)?;
                        commands::ai_center::run_model_pricing_set(&targets, &from_file, output)
                            .await?;
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
                        confirm_destructive("Create a new API key?", yes, agent_mode)?;
                        commands::api_keys::run_create(&targets, &from_file, output).await?;
                    }
                    ApiKeysCmd::Update { from_file, id } => {
                        confirm_destructive(&format!("Update API key '{id}'?"), yes, agent_mode)?;
                        commands::api_keys::run_update(&targets, &id, &from_file, output).await?;
                    }
                    ApiKeysCmd::Delete { id } => {
                        confirm_destructive(&format!("Delete API key '{id}'?"), yes, agent_mode)?;
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
                            confirm_destructive("Bulk delete API keys?", yes, agent_mode)?;
                            commands::api_keys::run_admin_delete(&targets, &ids).await?;
                        }
                        ApiKeysAdminCmd::SetStatus { ids, active } => {
                            confirm_destructive(
                                &format!("Set API key status to active={active}?"),
                                yes,
                                agent_mode,
                            )?;
                            commands::api_keys::run_admin_set_status(&targets, &ids, active)
                                .await?;
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
                        confirm_destructive("Create a new custom role?", yes, agent_mode)?;
                        commands::roles::run_create(&targets, &from_file, output).await?;
                    }
                    RolesCmd::Update { from_file, id } => {
                        confirm_destructive(&format!("Update role '{id}'?"), yes, agent_mode)?;
                        commands::roles::run_update(&targets, &id, &from_file, output).await?;
                    }
                    RolesCmd::Delete { id } => {
                        confirm_destructive(&format!("Delete role '{id}'?"), yes, agent_mode)?;
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
                        confirm_destructive("Create a new scope?", yes, agent_mode)?;
                        commands::scopes::run_create(&targets, &from_file, output).await?;
                    }
                    ScopesCmd::Update { from_file } => {
                        confirm_destructive("Update scope?", yes, agent_mode)?;
                        commands::scopes::run_update(&targets, &from_file, output).await?;
                    }
                    ScopesCmd::Delete { id } => {
                        confirm_destructive(&format!("Delete scope '{id}'?"), yes, agent_mode)?;
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
                        confirm_destructive("Create user(s)?", yes, agent_mode)?;
                        commands::users::run_create(&targets, &from_file, output).await?;
                    }
                    UsersCmd::Update { from_file } => {
                        confirm_destructive("Update user(s)?", yes, agent_mode)?;
                        commands::users::run_update(&targets, &from_file, output).await?;
                    }
                    UsersCmd::SetStatus { user_ids, status } => {
                        confirm_destructive(
                            &format!("Set user status to '{status}'?"),
                            yes,
                            agent_mode,
                        )?;
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
                        confirm_destructive("Create a new team group?", yes, agent_mode)?;
                        commands::team_groups::run_create(&targets, &from_file, output).await?;
                    }
                    TeamGroupsCmd::Update { from_file, id } => {
                        confirm_destructive(
                            &format!("Update team group '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::team_groups::run_update(&targets, &id, &from_file, output)
                            .await?;
                    }
                    TeamGroupsCmd::Delete { id } => {
                        confirm_destructive(
                            &format!("Delete team group '{id}'?"),
                            yes,
                            agent_mode,
                        )?;
                        commands::team_groups::run_delete(&targets, &id).await?;
                    }
                },
                IamCmd::IpAccess { cmd } => match cmd {
                    IpAccessCmd::Get => {
                        commands::ip_access::run_get(&targets, output).await?;
                    }
                    IpAccessCmd::Create { from_file } => {
                        confirm_destructive("Create IP access rules?", yes, agent_mode)?;
                        commands::ip_access::run_create(&targets, &from_file, output).await?;
                    }
                    IpAccessCmd::Update { from_file } => {
                        confirm_destructive("Update IP access rules?", yes, agent_mode)?;
                        commands::ip_access::run_update(&targets, &from_file, output).await?;
                    }
                    IpAccessCmd::Delete => {
                        confirm_destructive(
                            "Delete all IP access rules? This removes all IP restrictions.",
                            yes,
                            agent_mode,
                        )?;
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
                        confirm_destructive(
                            "Create metrics archive configuration?",
                            yes,
                            agent_mode,
                        )?;
                        commands::data_archive::run_metrics_create(&targets, &from_file, output)
                            .await?;
                    }
                    DataArchiveMetricsCmd::Update { from_file } => {
                        confirm_destructive(
                            "Update metrics archive configuration?",
                            yes,
                            agent_mode,
                        )?;
                        commands::data_archive::run_metrics_update(&targets, &from_file, output)
                            .await?;
                    }
                    DataArchiveMetricsCmd::Enable => {
                        confirm_destructive("Enable metrics archiving?", yes, agent_mode)?;
                        commands::data_archive::run_metrics_enable(&targets).await?;
                    }
                    DataArchiveMetricsCmd::Disable => {
                        confirm_destructive("Disable metrics archiving?", yes, agent_mode)?;
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
                        confirm_destructive("Set logs archive target?", yes, agent_mode)?;
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
                    confirm_destructive("Create a new SLO?", yes, agent_mode)?;
                    commands::slos::run_create(&targets, &from_file, output).await?;
                }
                SlosCmd::Update { from_file } => {
                    confirm_destructive("Update SLO?", yes, agent_mode)?;
                    commands::slos::run_update(&targets, &from_file, output).await?;
                }
                SlosCmd::Delete { id } => {
                    confirm_destructive(&format!("Delete SLO '{id}'?"), yes, agent_mode)?;
                    commands::slos::run_delete(&targets, &id).await?;
                }
            },

            Commands::Infra { cmd } => match cmd {
                InfraCmd::Resources { cmd } => match cmd {
                    InfraResourcesCmd::Types => {
                        commands::infra::run_types(&targets, output).await?;
                    }
                    InfraResourcesCmd::List {
                        category,
                        r#type,
                        name_filter,
                        scope,
                        start_row,
                        end_row,
                    } => {
                        commands::infra::run_list(
                            &targets,
                            &category,
                            &r#type,
                            name_filter.as_deref(),
                            &scope,
                            start_row,
                            end_row,
                            output,
                        )
                        .await?;
                    }
                    InfraResourcesCmd::HealthHistory { resource_id } => {
                        commands::infra::run_health_history(&targets, &resource_id, output)
                            .await?;
                    }
                    InfraResourcesCmd::RawData { resource_id } => {
                        commands::infra::run_raw_data(&targets, &resource_id, output).await?;
                    }
                },
            },

            Commands::ServiceCatalog { cmd } => match cmd {
                ServiceCatalogCmd::EntityTypes => {
                    commands::service_catalog::run_entity_types(&targets, output).await?;
                }
                ServiceCatalogCmd::Schema { entity_type } => {
                    commands::service_catalog::run_schema(&targets, &entity_type, output).await?;
                }
                ServiceCatalogCmd::Entities { entity_type } => {
                    commands::service_catalog::run_entities(&targets, &entity_type, output)
                        .await?;
                }
                ServiceCatalogCmd::Data {
                    entity_type,
                    start,
                    end,
                    columns,
                    group_by,
                    filters,
                    aggregation,
                    limit,
                    sort_column,
                    sort_order,
                } => {
                    commands::service_catalog::run_data(
                        &targets,
                        &entity_type,
                        &start,
                        &end,
                        &columns,
                        &group_by,
                        &filters,
                        &aggregation,
                        limit,
                        sort_column.as_deref(),
                        sort_order.as_deref(),
                        output,
                    )
                    .await?;
                }
                ServiceCatalogCmd::EntityData {
                    entity_type,
                    entity_id,
                    start,
                    end,
                    columns,
                    group_by,
                    filters,
                    aggregation,
                } => {
                    commands::service_catalog::run_entity_data(
                        &targets,
                        &entity_type,
                        &entity_id,
                        &start,
                        &end,
                        &columns,
                        &group_by,
                        &filters,
                        &aggregation,
                        output,
                    )
                    .await?;
                }
            },

            Commands::SearchFields {
                text,
                search_type,
                dataset,
                limit,
                offset,
            } => match search_type {
                SearchType::Semantic => {
                    let dataset_str = match dataset {
                        SearchByValueDataset::Logs => "logs",
                        SearchByValueDataset::Spans => "spans",
                        SearchByValueDataset::All => {
                            bail!("--dataset all is only valid with -s value");
                        }
                    };
                    commands::search_fields::run(&targets, &text, dataset_str, limit, output)
                        .await?;
                }
                SearchType::Value => {
                    let dataset_str = match dataset {
                        SearchByValueDataset::Logs => "logs",
                        SearchByValueDataset::Spans => "spans",
                        SearchByValueDataset::All => "all",
                    };
                    commands::search_by_value::run(
                        &targets,
                        &text,
                        dataset_str,
                        limit,
                        offset,
                        output,
                    )
                    .await?;
                }
            },

            Commands::Olly { cmd } => match cmd {
                OllyCmd::Ask {
                    message,
                    chat_id,
                    model,
                    timeout,
                    agent_to_agent_mode,
                } => {
                    commands::olly::run_ask(
                        &targets,
                        &message,
                        chat_id.as_deref(),
                        &model,
                        timeout,
                        agent_to_agent_mode,
                        output,
                    )
                    .await?;
                }
                OllyCmd::Artifacts { cmd } => match cmd {
                    OllyArtifactsCmd::List => {
                        commands::olly::run_artifacts_list(&targets, output).await?;
                    }
                    OllyArtifactsCmd::Get { artifact_id } => {
                        commands::olly::run_artifacts_get(
                            &targets,
                            &artifact_id,
                            output,
                            max_direct,
                            &temp_dir,
                        )
                        .await?;
                    }
                },
            },
        } // end match cli.command
        Ok::<(), anyhow::Error>(())
    }
    .await;

    // Print update notice after command output so it doesn't scroll off.
    // Using a separate result variable (rather than ?) ensures the notice
    // fires even when the command returns an error — same behaviour as `gh`.
    if output == OutputFormat::Toon {
        update_check::maybe_print_toon_meta();
    } else {
        update_check::maybe_print_notice(output);
    }

    cmd_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_completions_shell_accepts_supported_shells() {
        assert_eq!(parse_completions_shell("zsh").unwrap(), Shell::Zsh);
        assert_eq!(parse_completions_shell("bash").unwrap(), Shell::Bash);
        assert_eq!(parse_completions_shell("fish").unwrap(), Shell::Fish);
    }

    #[test]
    fn parse_completions_shell_rejects_elvish() {
        // Elvish is a valid clap_complete Shell variant but cx has no adapter
        // for it, so the `cx init --install-completions` flag must reject it
        // up front rather than fail later at registration time.
        let err = parse_completions_shell("elvish").unwrap_err();
        assert!(err.contains("elvish"), "error should name the bad shell");
        assert!(
            err.contains("zsh") && err.contains("bash") && err.contains("fish"),
            "error should list the supported shells"
        );
    }

    #[test]
    fn parse_completions_shell_rejects_powershell_without_path() {
        // PowerShell has no default install path, so it isn't offered by the
        // flag (the interactive picker's "Other" + explicit path covers it).
        assert!(parse_completions_shell("powershell").is_err());
    }

    #[test]
    fn parse_completions_shell_rejects_garbage() {
        assert!(parse_completions_shell("not-a-shell").is_err());
    }
}
