use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let skills_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("skills");
    println!("cargo:rerun-if-changed={}", skills_dir.display());

    let mut skill_names = fs::read_dir(skills_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .path()
                .join("SKILL.md")
                .is_file()
                .then(|| entry.file_name().into_string().ok())
                .flatten()
        })
        .collect::<Vec<_>>();
    skill_names.sort();

    let generated = format!(
        "pub const BUNDLED_CORALOGIX_SKILL_NAMES: &[&str] = &[{}];",
        skill_names
            .iter()
            .map(|name| format!("{name:?}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let output = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bundled_skills.rs");
    fs::write(output, generated).unwrap();

    // When this binary was built, as Unix seconds. The update check treats
    // skills refreshed after this time as current: they come from the repo's
    // default branch, so they already match this release. Honors
    // SOURCE_DATE_EPOCH for reproducible builds.
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let built_at = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });
    println!("cargo:rustc-env=CX_BUILD_TIME={built_at}");
}
