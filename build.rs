fn main() {
    println!("cargo:rerun-if-changed=src/condition.lalrpop");
    lalrpop::process_root().unwrap();

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let generated = std::path::Path::new(&out_dir).join("embedded_deps_sidecar.rs");
    let sidecar = std::path::Path::new("tools")
        .join("deps-extractor")
        .join("dist")
        .join("wave-analyzer-deps-extractor.exe");

    println!("cargo:rerun-if-changed={}", sidecar.display());

    let content = if sidecar.exists() {
        format!(
            "pub const EMBEDDED_DEPS_SIDECAR: Option<&[u8]> = Some(include_bytes!(r#\"{}\"#));\n",
            sidecar.canonicalize().unwrap().display()
        )
    } else {
        "pub const EMBEDDED_DEPS_SIDECAR: Option<&[u8]> = None;\n".to_string()
    };

    std::fs::write(generated, content).unwrap();
}
