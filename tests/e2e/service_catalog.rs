use std::sync::OnceLock;

use crate::harness;

#[test]
#[ignore]
fn service_catalog_entity_types() {
    if harness::require_creds("service_catalog_entity_types").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["service-catalog", "entity-types", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["entity_type", "id", "display_name"]);
}

#[test]
#[ignore]
fn service_catalog_schema() {
    if harness::require_creds("service_catalog_schema").is_none() {
        return;
    }
    let Some(entity_type) = discover_entity_type() else {
        eprintln!("[e2e] skipping service_catalog_schema: no entity types on test team");
        return;
    };
    let v = harness::run_ok_json(&["service-catalog", "schema", &entity_type, "-o", "json"]);
    harness::assert_object_with_keys(&v, &["columns", "groupable_labels", "filterable_labels"]);
    harness::assert_array_of_objects_with_keys(&v["columns"], &["id", "display_name"]);
}

#[test]
#[ignore]
fn service_catalog_entities() {
    if harness::require_creds("service_catalog_entities").is_none() {
        return;
    }
    let Some(entity_type) = discover_entity_type() else {
        eprintln!("[e2e] skipping service_catalog_entities: no entity types on test team");
        return;
    };
    let v = harness::run_ok_json(&["service-catalog", "entities", &entity_type, "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["name", "system", "last_seen"]);
}

#[test]
#[ignore]
fn service_catalog_data() {
    if harness::require_creds("service_catalog_data").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!("[e2e] skipping service_catalog_data: no entity types/columns on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        harness::SHORT_WINDOW_START,
        "--end",
        "now",
        "--column",
        &column,
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["rows", "columns"]);
    harness::assert_array(&v["rows"]);
}

#[test]
#[ignore]
fn service_catalog_entity_data() {
    if harness::require_creds("service_catalog_entity_data").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!(
            "[e2e] skipping service_catalog_entity_data: no entity types/columns on test team"
        );
        return;
    };
    let Some(entity_id) = discover_entity_id(&entity_type) else {
        eprintln!(
            "[e2e] skipping service_catalog_entity_data: no entities of type '{entity_type}' on test team"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "entity-data",
        &entity_type,
        &entity_id,
        "--start",
        harness::SHORT_WINDOW_START,
        "--end",
        "now",
        "--column",
        &column,
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["rows", "columns"]);
}

/// Setup insights only: deployment insights query metrics around every
/// service's last change and are much slower.
#[test]
#[ignore]
fn service_catalog_insights() {
    if harness::require_creds("service_catalog_insights").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "service-catalog",
        "insights",
        "service",
        "--exclude-category",
        "deployment",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["perEntityType"]);
    // One entity type requested, one entry back.
    assert_eq!(
        v["perEntityType"][0]["entityType"], "ENTITY_TYPE_SERVICE",
        "{v}"
    );
}

#[test]
#[ignore]
fn service_catalog_entity_insights() {
    if harness::require_creds("service_catalog_entity_insights").is_none() {
        return;
    }
    let Some(service_name) = discover_entity_id("service") else {
        eprintln!("[e2e] skipping service_catalog_entity_insights: no services on test team");
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "entity-insights",
        "service",
        &service_name,
        "--exclude-category",
        "deployment",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &[]);
}

/// The API accepts `archiveFilter`. What it filters depends on the team's
/// archive, so only the response shape is checked.
#[test]
#[ignore]
fn service_catalog_insights_archived_include() {
    if harness::require_creds("service_catalog_insights_archived_include").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "service-catalog",
        "insights",
        "service",
        "--exclude-category",
        "deployment",
        "--archived",
        "include",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["perEntityType"]);
    assert_eq!(
        v["perEntityType"][0]["entityType"], "ENTITY_TYPE_SERVICE",
        "{v}"
    );
}

#[test]
#[ignore]
fn service_catalog_entity_insights_archived_only() {
    if harness::require_creds("service_catalog_entity_insights_archived_only").is_none() {
        return;
    }
    let Some(service_name) = discover_entity_id("service") else {
        eprintln!(
            "[e2e] skipping service_catalog_entity_insights_archived_only: no services on test team"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "entity-insights",
        "service",
        &service_name,
        "--exclude-category",
        "deployment",
        "--archived",
        "only",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &[]);
}

/// Drills into the first setup insight that fired on the test team.
#[test]
#[ignore]
fn service_catalog_insight_drilldown() {
    if harness::require_creds("service_catalog_insight_drilldown").is_none() {
        return;
    }
    let listed = setup_insights();
    harness::assert_object_with_keys(listed, &["perEntityType"]);
    let Some(insight_type) = first_unscoped_insight_type(&listed["perEntityType"][0]["insights"])
    else {
        eprintln!(
            "[e2e] not covered: service-catalog insight-drilldown - no setup insight fired \
             on the test team"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "insight-drilldown",
        &insight_type,
        "service",
        "--page-size",
        "3",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["insight"]);
    assert_eq!(v["insight"]["type"], insight_type.as_str(), "{v}");
    assert!(v["entities"].as_array().map_or(0, Vec::len) <= 3, "{v}");
}

/// Drills into the first setup insight of one service, per environment.
#[test]
#[ignore]
fn service_catalog_entity_insight_drilldown() {
    if harness::require_creds("service_catalog_entity_insight_drilldown").is_none() {
        return;
    }
    let Some(service_name) = discover_entity_id("service") else {
        eprintln!(
            "[e2e] skipping service_catalog_entity_insight_drilldown: no services on test team"
        );
        return;
    };
    let listed = harness::run_ok_json(&[
        "service-catalog",
        "entity-insights",
        "service",
        &service_name,
        "--exclude-category",
        "deployment",
        "-o",
        "json",
    ]);
    let Some(insight_type) = first_unscoped_insight_type(&listed["insights"]) else {
        eprintln!(
            "[e2e] not covered: service-catalog entity-insight-drilldown - no setup insight \
             fired for {service_name}"
        );
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "entity-insight-drilldown",
        &insight_type,
        "service",
        &service_name,
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["insight"]);
    assert_eq!(v["insight"]["type"], insight_type.as_str(), "{v}");
}

/// `--breakdown` / `--top-values` / `--breakdown-sort count` shape every
/// breakdown, and `--filter deployment_environment_name=<env>` (an
/// environment taken from that breakdown) narrows it to that environment.
#[test]
#[ignore]
fn service_catalog_insights_breakdowns_and_filter() {
    if harness::require_creds("service_catalog_insights_breakdowns_and_filter").is_none() {
        return;
    }
    let broken_down = harness::run_ok_json(&[
        "service-catalog",
        "insights",
        "service",
        "--exclude-category",
        "deployment",
        "--breakdown",
        "environment",
        "--top-values",
        "2",
        "--breakdown-sort",
        "count",
        "-o",
        "json",
    ]);
    let breakdowns = environment_breakdowns(&broken_down);
    for breakdown in &breakdowns {
        let counts: Vec<i64> = breakdown["topValues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|top_value| top_value["insightCount"].as_i64().unwrap_or(0))
            .collect();
        assert!(counts.len() <= 2, "--top-values 2: {breakdown}");
        assert!(
            counts.windows(2).all(|pair| pair[0] >= pair[1]),
            "--breakdown-sort count: {breakdown}"
        );
    }
    let Some(environment) = breakdowns
        .iter()
        .find_map(|breakdown| breakdown["topValues"][0]["value"].as_str())
    else {
        eprintln!(
            "[e2e] not covered: service-catalog insights --breakdown/--filter - no environment \
             breakdown on the test team"
        );
        return;
    };

    let filter = format!("deployment_environment_name={environment}");
    let filtered = harness::run_ok_json(&[
        "service-catalog",
        "insights",
        "service",
        "--exclude-category",
        "deployment",
        "--breakdown",
        "environment",
        "--filter",
        &filter,
        "-o",
        "json",
    ]);
    let filtered_breakdowns = environment_breakdowns(&filtered);
    assert!(!filtered_breakdowns.is_empty(), "{filtered}");
    for breakdown in filtered_breakdowns {
        for top_value in breakdown["topValues"].as_array().unwrap() {
            assert_eq!(
                top_value["value"], environment,
                "--filter {filter}: {breakdown}"
            );
        }
    }

    let text = harness::run_ok_nonempty(&[
        "service-catalog",
        "insights",
        "service",
        "--exclude-category",
        "deployment",
        "--breakdown",
        "environment",
        "--filter",
        &filter,
        "-o",
        "text",
    ]);
    let text = String::from_utf8_lossy(&text);
    assert!(text.contains("Breakdowns:"), "{text}");
    assert!(
        text.contains(&format!("environment (1 value): {environment} ")),
        "{text}"
    );
}

/// `--sort name` with `--sort-order` and `--page-index`/`--page-size`, checked
/// against the insight's whole list rather than a local collation, which
/// may order punctuation differently from the backend.
#[test]
#[ignore]
fn service_catalog_insight_drilldown_sort_and_pages() {
    if harness::require_creds("service_catalog_insight_drilldown_sort_and_pages").is_none() {
        return;
    }
    let Some(insight_type) =
        first_setup_insight_type(|insight| affected_entity_count(insight) <= 200)
    else {
        eprintln!(
            "[e2e] not covered: service-catalog insight-drilldown --sort/--page-index - no setup \
             insight affecting at most 200 services on the test team"
        );
        return;
    };
    let ascending = drilldown_json(
        &insight_type,
        &[
            "--sort",
            "name",
            "--sort-order",
            "asc",
            "--page-size",
            "200",
        ],
    );
    let total = ascending["totalEntities"].as_i64().unwrap_or(0);
    let ascending_names = entity_names(&ascending);
    assert_eq!(ascending_names.len() as i64, total, "{ascending}");

    let descending = drilldown_json(
        &insight_type,
        &[
            "--sort",
            "name",
            "--sort-order",
            "desc",
            "--page-size",
            "200",
        ],
    );
    let mut reversed = entity_names(&descending);
    reversed.reverse();
    assert_eq!(reversed, ascending_names, "desc must reverse asc");

    let second_page = drilldown_json(
        &insight_type,
        &[
            "--sort",
            "name",
            "--sort-order",
            "asc",
            "--page-size",
            "2",
            "--page-index",
            "1",
        ],
    );
    assert_eq!(second_page["totalEntities"], total, "{second_page}");
    let page_end = ascending_names.len().min(4);
    assert_eq!(
        entity_names(&second_page),
        ascending_names[ascending_names.len().min(2)..page_end],
        "--page-index 1 --page-size 2"
    );

    let past_the_end = ((total - 1) / 2 + 1).to_string();
    let text = harness::run_ok_nonempty(&[
        "service-catalog",
        "insight-drilldown",
        &insight_type,
        "service",
        "--page-size",
        "2",
        "--page-index",
        &past_the_end,
        "-o",
        "text",
    ]);
    let text = String::from_utf8_lossy(&text);
    assert!(
        text.contains(&format!(
            "No entities on --page-index {past_the_end}: {total} in total"
        )),
        "{text}"
    );
}

/// `--name-search`, `--environment-match every` and `--filter
/// deployment_environment_name` each narrow the entities of one insight;
/// `--scope language=<name>` drills into a per-language insight.
#[test]
#[ignore]
fn service_catalog_insight_drilldown_narrowing() {
    if harness::require_creds("service_catalog_insight_drilldown_narrowing").is_none() {
        return;
    }
    let Some(insight_type) = first_setup_insight_type(|_| true) else {
        eprintln!(
            "[e2e] not covered: service-catalog insight-drilldown --name-search/--filter - no \
             setup insight fired on the test team"
        );
        return;
    };
    let first_page = drilldown_json(&insight_type, &["--page-size", "20"]);
    let name = first_page["entities"][0]["entity"]["name"]
        .as_str()
        .unwrap_or_default();
    assert!(!name.is_empty(), "{first_page}");

    let search = name.to_uppercase();
    let searched = drilldown_json(&insight_type, &["--name-search", &search]);
    let searched_names = entity_names(&searched);
    assert!(searched_names.iter().any(|n| n == name), "{searched}");
    for searched_name in &searched_names {
        assert!(
            searched_name
                .to_lowercase()
                .contains(&search.to_lowercase()),
            "--name-search {search}: {searched_name}"
        );
    }

    let every = drilldown_json(&insight_type, &["--environment-match", "every"]);
    for entity in every["entities"].as_array().unwrap() {
        assert_eq!(
            entity["affectedEnvironmentCount"], entity["environmentCount"],
            "--environment-match every: {entity}"
        );
    }
    assert!(
        every["totalEntities"].as_i64().unwrap_or(0)
            <= first_page["totalEntities"].as_i64().unwrap_or(0),
        "{every}"
    );

    if let Some((_, environment)) = affected_environment_of(&first_page) {
        let filter = format!("deployment_environment_name={environment}");
        let filtered = drilldown_json(&insight_type, &["--filter", &filter]);
        let filtered_entities = filtered["entities"].as_array().unwrap();
        assert!(!filtered_entities.is_empty(), "{filtered}");
        for entity in filtered_entities {
            assert_eq!(
                entity["affectedEnvironments"],
                serde_json::json!([environment]),
                "--filter {filter}: {entity}"
            );
        }
    } else {
        eprintln!(
            "[e2e] not covered: service-catalog insight-drilldown --filter - no entity of \
             {insight_type} is affected in a known environment"
        );
    }

    let language = setup_insights()["perEntityType"][0]["insights"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|insight| insight["type"] == "INSIGHT_TYPE_MISSING_RUNTIME_METRICS")
        .find_map(|insight| insight["language"]["name"].as_str());
    let Some(language) = language else {
        eprintln!(
            "[e2e] not covered: service-catalog insight-drilldown --scope - no \
             missing-runtime-metrics insight on the test team"
        );
        return;
    };
    let scope = format!("language={language}");
    let scoped = drilldown_json(
        "missing-runtime-metrics",
        &["--scope", &scope, "--page-size", "1"],
    );
    assert_eq!(scoped["insight"]["language"]["name"], language, "{scoped}");
}

/// `--environment` narrows `entity-insights` and `entity-insight-drilldown`
/// to one environment, and `--dimension owner` lists owners instead.
#[test]
#[ignore]
fn service_catalog_entity_insights_environment_and_dimension() {
    if harness::require_creds("service_catalog_entity_insights_environment_and_dimension").is_none()
    {
        return;
    }
    let Some(insight_type) = first_setup_insight_type(|_| true) else {
        eprintln!(
            "[e2e] not covered: service-catalog entity-insights --environment - no setup insight \
             fired on the test team"
        );
        return;
    };
    let page = drilldown_json(&insight_type, &["--page-size", "20"]);
    if let Some((service_name, environment)) = affected_environment_of(&page) {
        let insights = harness::run_ok_json(&[
            "service-catalog",
            "entity-insights",
            "service",
            service_name,
            "--exclude-category",
            "deployment",
            "--environment",
            environment,
            "-o",
            "json",
        ]);
        let listed = insights["insights"].as_array().unwrap();
        assert!(
            listed
                .iter()
                .any(|insight| insight["type"] == insight_type.as_str()),
            "{insights}"
        );
        for insight in listed {
            for insight_environment in insight["environments"].as_array().into_iter().flatten() {
                assert_eq!(
                    insight_environment, environment,
                    "--environment {environment}: {insight}"
                );
            }
        }

        let by_environment = harness::run_ok_json(&[
            "service-catalog",
            "entity-insight-drilldown",
            &insight_type,
            "service",
            service_name,
            "--environment",
            environment,
            "-o",
            "json",
        ]);
        assert_eq!(by_environment["dimension"], "INSIGHT_DIMENSION_ENVIRONMENT");
        let values = by_environment["values"].as_array().unwrap();
        assert!(!values.is_empty(), "{by_environment}");
        for value in values {
            assert_eq!(
                value["value"], environment,
                "--environment {environment}: {value}"
            );
        }
    } else {
        eprintln!(
            "[e2e] not covered: service-catalog entity-insights --environment - no entity of \
             {insight_type} is affected in a known environment"
        );
    }

    // An insight type has an owner breakdown only if the owner dimension
    // applies to it, as `--dimension owner` needs.
    let owner_listing = harness::run_ok_json(&[
        "service-catalog",
        "insights",
        "service",
        "--exclude-category",
        "deployment",
        "--breakdown",
        "owner",
        "-o",
        "json",
    ]);
    let with_owner_breakdown: Vec<serde_json::Value> = owner_listing["perEntityType"][0]
        ["insights"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|insight| {
            insight["breakdowns"]
                .as_array()
                .is_some_and(|breakdowns| !breakdowns.is_empty())
        })
        .cloned()
        .collect();
    let Some(owner_insight_type) =
        first_unscoped_insight_type(&serde_json::Value::Array(with_owner_breakdown))
    else {
        eprintln!(
            "[e2e] not covered: service-catalog entity-insight-drilldown --dimension - no setup \
             insight with an owner breakdown on the test team"
        );
        return;
    };
    let owner_page = drilldown_json(&owner_insight_type, &["--page-size", "1"]);
    let service_name = owner_page["entities"][0]["entity"]["name"]
        .as_str()
        .unwrap_or_default();
    assert!(!service_name.is_empty(), "{owner_page}");
    let by_owner = harness::run_ok_json(&[
        "service-catalog",
        "entity-insight-drilldown",
        &owner_insight_type,
        "service",
        service_name,
        "--dimension",
        "owner",
        "-o",
        "json",
    ]);
    assert_eq!(
        by_owner["dimension"], "INSIGHT_DIMENSION_OWNER",
        "{by_owner}"
    );
    assert_eq!(
        by_owner["insight"]["type"],
        owner_insight_type.as_str(),
        "{by_owner}"
    );
}

/// `--system` names a database for `entity-insights` and
/// `entity-insight-drilldown`.
#[test]
#[ignore]
fn service_catalog_database_entity_insights_with_system() {
    if harness::require_creds("service_catalog_database_entity_insights_with_system").is_none() {
        return;
    }
    let Some((database, system)) = discover_database() else {
        eprintln!(
            "[e2e] not covered: service-catalog entity-insights --system - no databases on the \
             test team"
        );
        return;
    };
    let database_insights = harness::run_ok_json(&[
        "service-catalog",
        "entity-insights",
        "database",
        &database,
        "--system",
        &system,
        "--exclude-category",
        "deployment",
        "-o",
        "json",
    ]);
    let Some(insight_type) = first_unscoped_insight_type(&database_insights["insights"]) else {
        eprintln!(
            "[e2e] not covered: service-catalog entity-insight-drilldown --system - no setup \
             insight fired for database {database}"
        );
        return;
    };
    let drilldown = harness::run_ok_json(&[
        "service-catalog",
        "entity-insight-drilldown",
        &insight_type,
        "database",
        &database,
        "--system",
        &system,
        "-o",
        "json",
    ]);
    assert_eq!(
        drilldown["insight"]["type"],
        insight_type.as_str(),
        "{drilldown}"
    );
}

/// Archives an insight type that is not archived yet and unarchives it again,
/// restoring the team's archive. Only opt-in types are candidates: no default
/// `insights` request returns them, so the other tests never see the change.
#[test]
#[ignore]
fn service_catalog_insight_types_archive_unarchive() {
    if harness::require_write_creds("service_catalog_insight_types_archive_unarchive").is_none() {
        return;
    }
    let archived_now = harness::run_ok_json(&[
        "service-catalog",
        "insights",
        "service",
        "--exclude-category",
        "deployment",
        "--archived",
        "include",
        "-o",
        "json",
    ]);
    if archived_now["archiveUnavailable"] == true {
        eprintln!(
            "[e2e] skipping service_catalog_insight_types_archive_unarchive: archive unavailable"
        );
        return;
    }
    let is_archived = |v: &serde_json::Value, proto: &str| {
        v["archivedTypes"]
            .as_array()
            .is_some_and(|types| types.iter().any(|t| t == proto))
    };
    let Some((short, proto)) = [
        ("mix-shift", "INSIGHT_TYPE_MIX_SHIFT"),
        ("traffic-drop", "INSIGHT_TYPE_TRAFFIC_DROP"),
        (
            "dependency-regression",
            "INSIGHT_TYPE_DEPENDENCY_REGRESSION",
        ),
    ]
    .into_iter()
    .find(|(_, proto)| !is_archived(&archived_now, proto)) else {
        eprintln!(
            "[e2e] not covered: service-catalog insight-types archive - every candidate type \
             is already archived on the test team"
        );
        return;
    };

    let archived = harness::run_write_json(&[
        "service-catalog",
        "insight-types",
        "archive",
        "--type",
        short,
        "-o",
        "json",
    ]);
    let mut cleanup = harness::Cleanup::new(&[
        "service-catalog",
        "insight-types",
        "unarchive",
        "--type",
        short,
    ]);
    assert!(is_archived(&archived, proto), "{archived}");

    let unarchived = harness::run_write_json(&[
        "service-catalog",
        "insight-types",
        "unarchive",
        "--type",
        short,
        "-o",
        "json",
    ]);
    cleanup.disarm();
    assert!(!is_archived(&unarchived, proto), "{unarchived}");
}

/// The type of the first insight without a `scope`, which a drilldown can
/// request without `--scope`.
fn first_unscoped_insight_type(insights: &serde_json::Value) -> Option<String> {
    insights.as_array()?.iter().find_map(|insight| {
        let scoped = ["language", "operation", "dependency", "requestKind"]
            .iter()
            .any(|field| insight.get(*field).is_some());
        if scoped {
            return None;
        }
        insight.get("type")?.as_str().map(String::from)
    })
}

/// The setup insights of services. Cached across the insight tests.
fn setup_insights() -> &'static serde_json::Value {
    static CACHE: OnceLock<serde_json::Value> = OnceLock::new();
    CACHE.get_or_init(|| {
        harness::run_ok_json(&[
            "service-catalog",
            "insights",
            "service",
            "--exclude-category",
            "deployment",
            "-o",
            "json",
        ])
    })
}

/// The first unscoped setup insight of services that `accept` keeps.
fn first_setup_insight_type(accept: impl Fn(&serde_json::Value) -> bool) -> Option<String> {
    let accepted: Vec<serde_json::Value> = setup_insights()["perEntityType"][0]["insights"]
        .as_array()?
        .iter()
        .filter(|insight| accept(insight))
        .cloned()
        .collect();
    first_unscoped_insight_type(&serde_json::Value::Array(accepted))
}

/// The services an insight affects in at least one environment; 0 for a cap.
fn affected_entity_count(insight: &serde_json::Value) -> i64 {
    insight
        .pointer("/entities/insightCount")
        .or_else(|| insight.pointer("/environmentCoverage/atLeastOne/insightCount"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
}

fn drilldown_json(insight_type: &str, extra_args: &[&str]) -> serde_json::Value {
    let mut args = vec![
        "service-catalog",
        "insight-drilldown",
        insight_type,
        "service",
    ];
    args.extend_from_slice(extra_args);
    args.extend_from_slice(&["-o", "json"]);
    harness::run_ok_json(&args)
}

/// `(name, environment)` of the first entity on a drilldown page affected in
/// a named environment: an entity with no environment label has none.
fn affected_environment_of(page: &serde_json::Value) -> Option<(&str, &str)> {
    page["entities"].as_array()?.iter().find_map(|entity| {
        let name = entity["entity"]["name"].as_str()?;
        let environment = entity["affectedEnvironments"][0]
            .as_str()
            .filter(|environment| !environment.is_empty())?;
        Some((name, environment))
    })
}

fn environment_breakdowns(response: &serde_json::Value) -> Vec<&serde_json::Value> {
    response["perEntityType"][0]["insights"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|insight| insight["breakdowns"].as_array().into_iter().flatten())
        .inspect(|breakdown| {
            assert_eq!(
                breakdown["dimension"], "INSIGHT_DIMENSION_ENVIRONMENT",
                "only the requested dimension: {breakdown}"
            );
        })
        .collect()
}

fn entity_names(drilldown: &serde_json::Value) -> Vec<String> {
    drilldown["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entity| entity["entity"]["name"].as_str().map(String::from))
        .collect()
}

/// A database's `(name, system)`: databases are identified by both.
fn discover_database() -> Option<(String, String)> {
    let stdout = harness::run_ok(&["service-catalog", "entities", "database", "-o", "json"]);
    let v = harness::parse_json(&stdout)?;
    v.as_array()?.iter().find_map(|item| {
        let name = item.get("name")?.as_str()?;
        let system = item.get("system")?.as_str().filter(|s| !s.is_empty())?;
        Some((name.to_string(), system.to_string()))
    })
}

/// Discover an entity type from `service-catalog entity-types`. Cached so
/// multiple tests don't each pay for the call.
fn discover_entity_type() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("service_catalog_discover_entity_type")?;
            let stdout = harness::run_ok(&["service-catalog", "entity-types", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find_map(|item| item.get("entity_type")?.as_str().map(String::from))
        })
        .clone()
}

/// Discover an `(entity_type, column_id)` pair via `entity-types` + `schema`,
/// skipping entity types whose schema declares `required_filters` (e.g.
/// `transaction` requires `service_name`) since supplying those would need
/// another round of discovery this test isn't set up to do. Cached across
/// the `data` and `entity-data` tests.
fn discover_entity_type_and_column() -> Option<(String, String)> {
    static CACHE: OnceLock<Option<(String, String)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("service_catalog_discover_entity_type_and_column")?;
            let stdout = harness::run_ok(&["service-catalog", "entity-types", "-o", "json"]);
            let entity_types = harness::parse_json(&stdout)?;
            entity_types.as_array()?.iter().find_map(|item| {
                let entity_type = item.get("entity_type")?.as_str()?.to_string();
                let stdout =
                    harness::run_ok(&["service-catalog", "schema", &entity_type, "-o", "json"]);
                let schema = harness::parse_json(&stdout)?;
                let has_required_filters = schema
                    .get("required_filters")
                    .and_then(|v| v.as_array())
                    .is_some_and(|arr| !arr.is_empty());
                if has_required_filters {
                    return None;
                }
                let column = schema
                    .get("columns")?
                    .as_array()?
                    .iter()
                    .find(|item| !harness::is_e2e_item(item))?
                    .get("id")?
                    .as_str()?
                    .to_string();
                Some((entity_type, column))
            })
        })
        .clone()
}

/// Discover a known entity name for `entity_type` via `service-catalog
/// entities`. Not cached: each caller asks for its own entity type.
fn discover_entity_id(entity_type: &str) -> Option<String> {
    let stdout = harness::run_ok(&["service-catalog", "entities", entity_type, "-o", "json"]);
    let v = harness::parse_json(&stdout)?;
    v.as_array()?
        .iter()
        .find_map(|item| item.get("name")?.as_str().map(String::from))
}

/// `--aggregation timeseries` answers with one series per entity and column.
#[test]
#[ignore]
fn service_catalog_data_timeseries() {
    if harness::require_creds("service_catalog_data_timeseries").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!("[e2e] skipping service_catalog_data_timeseries: no entity types/columns");
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--aggregation",
        "timeseries",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["series"]);
    harness::assert_array_of_objects_with_keys(&v["series"], &["columnId", "labels", "datapoints"]);
    for series in v["series"].as_array().unwrap() {
        assert_eq!(series["columnId"], column, "{series}");
        // A bucket with no data carries only its timestamp.
        harness::assert_array_of_objects_with_keys(&series["datapoints"], &["timestampSeconds"]);
    }

    // The table-only controls are refused up front for timeseries.
    let output = harness::cx()
        .args([
            "service-catalog",
            "data",
            &entity_type,
            "--start",
            "now-30m",
            "--end",
            "now",
            "--column",
            &column,
            "--aggregation",
            "timeseries",
            "--limit",
            "2",
        ])
        .output()
        .expect("cx must run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only apply to --aggregation table"));
}

/// `--limit` / `--sort-column` / `--sort-order` rank and cap table rows; two
/// columns come back on every row and in the `columns` metadata.
#[test]
#[ignore]
fn service_catalog_data_table_sorted_and_limited() {
    if harness::require_creds("service_catalog_data_table_sorted_and_limited").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!("[e2e] skipping service_catalog_data_table_sorted_and_limited: no entity types");
        return;
    };
    let Some(second) = discover_second_column(&entity_type, &column) else {
        eprintln!("[e2e] skipping service_catalog_data_table_sorted_and_limited: only one column");
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--column",
        &second,
        "--aggregation",
        "table",
        "--limit",
        "2",
        "--sort-column",
        &column,
        "--sort-order",
        "desc",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["rows", "columns"]);
    let rows = v["rows"].as_array().unwrap();
    assert!(rows.len() <= 2, "--limit 2: {v}");
    for row in rows {
        assert!(
            row.get(&column).is_some() && row.get(&second).is_some(),
            "{row}"
        );
    }
    let ids: Vec<&str> = v["columns"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert_eq!(ids, [column.as_str(), second.as_str()]);
    let numeric: Vec<f64> = rows.iter().filter_map(|r| r[&column].as_f64()).collect();
    assert!(
        numeric.windows(2).all(|w| w[0] >= w[1]),
        "desc order: {numeric:?}"
    );

    let text = harness::run_ok_nonempty(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--limit",
        "2",
        "--sort-column",
        &column,
        "-o",
        "text",
    ]);
    assert!(!String::from_utf8_lossy(&text).trim().is_empty());
}

/// `--group-by` adds the label to every row's identity; `--filter` on the
/// same label keeps only rows with that value. Both for `data` and for
/// `entity-data` (timeseries).
#[test]
#[ignore]
fn service_catalog_data_group_by_and_filter() {
    if harness::require_creds("service_catalog_data_group_by_and_filter").is_none() {
        return;
    }
    let Some((entity_type, column)) = discover_entity_type_and_column() else {
        eprintln!("[e2e] skipping service_catalog_data_group_by_and_filter: no entity types");
        return;
    };
    let Some(label) = discover_group_and_filter_label(&entity_type) else {
        eprintln!("[e2e] skipping service_catalog_data_group_by_and_filter: no label is both groupable and filterable");
        return;
    };
    let grouped = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--group-by",
        &label,
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&grouped, &["rows", "columns"]);
    let rows = grouped["rows"].as_array().unwrap();
    // Entities without the label come back with an empty identity, so only
    // the rows that carry it can be checked.
    let Some(value) = rows
        .iter()
        .find_map(|r| r[&label].as_str().map(String::from))
    else {
        eprintln!("[e2e] skipping the filter half: no rows with a '{label}' value in the window");
        return;
    };

    let filter = format!("{label}={value}");
    let filtered = harness::run_ok_json(&[
        "service-catalog",
        "data",
        &entity_type,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--group-by",
        &label,
        "--filter",
        &filter,
        "-o",
        "json",
    ]);
    let rows = filtered["rows"].as_array().unwrap();
    assert!(!rows.is_empty(), "{filtered}");
    for row in rows {
        assert_eq!(
            row[&label], value,
            "--filter {filter} must keep only that value: {row}"
        );
    }

    let Some(entity_id) = discover_entity_id(&entity_type) else {
        return;
    };
    let v = harness::run_ok_json(&[
        "service-catalog",
        "entity-data",
        &entity_type,
        &entity_id,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "--group-by",
        &label,
        "--filter",
        &filter,
        "--aggregation",
        "timeseries",
        "-o",
        "json",
    ]);
    harness::assert_object_with_keys(&v, &["series"]);
    for series in v["series"].as_array().unwrap() {
        assert_eq!(series["labels"][&label], value, "{series}");
    }
    harness::run_ok(&[
        "service-catalog",
        "entity-data",
        &entity_type,
        &entity_id,
        "--start",
        "now-30m",
        "--end",
        "now",
        "--column",
        &column,
        "-o",
        "text",
    ]);
    harness::run_ok_nonempty(&["service-catalog", "entity-types", "-o", "text"]);
    harness::run_ok_nonempty(&["service-catalog", "schema", &entity_type, "-o", "toon"]);
}

/// Client-side validation runs before any request.
#[test]
#[ignore]
fn service_catalog_rejects_bad_flags_locally() {
    if harness::require_creds("service_catalog_rejects_bad_flags_locally").is_none() {
        return;
    }
    for (args, expected) in [
        (
            vec![
                "data", "service", "--start", "now", "--end", "now-1h", "--column", "p99",
            ],
            "must be strictly after",
        ),
        (
            vec![
                "data",
                "service",
                "--start",
                "now-1h",
                "--end",
                "now",
                "--column",
                "p99",
                "--aggregation",
                "pie",
            ],
            "unknown --aggregation",
        ),
        (
            vec![
                "data",
                "service",
                "--start",
                "now-1h",
                "--end",
                "now",
                "--column",
                "p99",
                "--filter",
                "no-equals",
            ],
            "invalid --filter",
        ),
        (vec!["schema", "cx-no-such-entity-type"], "entity type"),
        (
            vec!["insight-drilldown", "no-such-insight", "service"],
            "unknown insight type",
        ),
        (
            vec!["insights", "service", "--category", "capacity"],
            "unknown --category",
        ),
    ] {
        let output = harness::cx()
            .arg("service-catalog")
            .args(&args)
            .output()
            .expect("cx must run");
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.to_lowercase().contains(expected),
            "{args:?}: {stderr}"
        );
    }
}

/// A second column id of the same entity type (for multi-column requests).
fn discover_second_column(entity_type: &str, first: &str) -> Option<String> {
    let stdout = harness::run_ok(&["service-catalog", "schema", entity_type, "-o", "json"]);
    let schema = harness::parse_json(&stdout)?;
    schema
        .get("columns")?
        .as_array()?
        .iter()
        .filter_map(|c| c.get("id")?.as_str())
        .find(|id| *id != first)
        .map(String::from)
}

/// A label that can be used both with `--group-by` and `--filter`.
fn discover_group_and_filter_label(entity_type: &str) -> Option<String> {
    let stdout = harness::run_ok(&["service-catalog", "schema", entity_type, "-o", "json"]);
    let schema = harness::parse_json(&stdout)?;
    let filterable: Vec<String> = schema
        .get("filterable_labels")?
        .as_array()?
        .iter()
        .filter_map(|l| l.as_str().map(String::from))
        .collect();
    schema
        .get("groupable_labels")?
        .as_array()?
        .iter()
        .filter_map(|l| l.as_str())
        .find(|l| filterable.iter().any(|f| f == l))
        .map(String::from)
}

// The only mutating subcommands are `insight-types archive|unarchive`, covered
// by `service_catalog_insight_types_archive_unarchive`. Deployment insights are
// excluded from every insights test: they need a recent deployment on the
// test team and query metrics around each service's last change.
