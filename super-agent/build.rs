use glob::glob;
use std::env;
use std::fs;
use std::path::Path;

const REGISTRY_PATH: &str = "agent-type-registry/";
const GENERATED_REGISTRY_FILE: &str = "generated_agent_type_registry.rs";

fn main() {
    generate_agent_type_registry();
    // setup the env variable for the generated registry path
    println!("cargo:rustc-env=GENERATED_REGISTRY_FILE={GENERATED_REGISTRY_FILE}");
    // re-run only if the registry has changed
    println!("cargo:rerun-if-changed={REGISTRY_PATH}")
}

fn generate_agent_type_registry() {
    let current_dir = env::current_dir().unwrap();
    let registry_paths = glob(format!("{REGISTRY_PATH}**/*.yaml").as_str()).unwrap();

    // comma-separated `include_bytes!(<full-path>)` for each file in the registry
    let static_array_content = registry_paths
        .map(|entry| {
            let path = entry.unwrap();
            let full_path = Path::new(&current_dir).join(path).display().to_string();
            format!("include_bytes!(\"{full_path}\")")
        })
        .collect::<Vec<_>>()
        .join(", ");

    // build generated file content
    let contents =
        format!("pub const AGENT_TYPE_REGISTRY_FILES: &[&[u8]] = &[{static_array_content}];");

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join(GENERATED_REGISTRY_FILE);
    fs::write(dest_path, contents).unwrap();
}
