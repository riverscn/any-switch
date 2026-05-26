use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let builtin_dir = manifest_dir.join("src/app_definitions/builtin");
    println!("cargo:rerun-if-changed={}", builtin_dir.display());

    let mut entries = fs::read_dir(&builtin_dir)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", builtin_dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("failed to read builtin definition entry: {err}"))
                .path()
        })
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("yaml"))
        .collect::<Vec<_>>();
    entries.sort();

    if entries.is_empty() {
        panic!(
            "no builtin app definitions found in {}",
            builtin_dir.display()
        );
    }

    for path in &entries {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let mut generated = String::from("&[\n");
    for path in entries {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_else(|| panic!("invalid builtin definition path {}", path.display()));
        generated.push_str("    BuiltinDefinitionAsset {\n");
        generated.push_str(&format!("        name: {name:?},\n"));
        let relative = path.strip_prefix(&manifest_dir).unwrap_or(&path);
        generated.push_str(&format!(
            "        text: include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/\", {:?})),\n",
            relative.display().to_string()
        ));
        generated.push_str("    },\n");
    }
    generated.push_str("]\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("builtin_definitions.rs"), generated)
        .expect("failed to write generated builtin definitions");
}
