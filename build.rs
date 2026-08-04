use std::env;
use std::fs;
use std::path::PathBuf;

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
}
