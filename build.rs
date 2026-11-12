use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // The clap command tree needs close to 1 MB of stack in a debug build, the
    // Windows default for the main thread; other platforms give 8 MB.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let arg = match env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => "/STACK:8388608",
            _ => "-Wl,--stack,8388608",
        };
        println!("cargo:rustc-link-arg-bins={arg}");
    }

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
