include!("../../build_identity.rs");
fn main() {
    emit_build_identity();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../jp-app/Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let source = std::fs::read_to_string(manifest).unwrap();
    let version = source.lines().find_map(|line| line.strip_prefix("version = ")).unwrap().trim_matches('"');
    println!("cargo:rustc-env=JPOP_APP_VERSION={version}");
}
