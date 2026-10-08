// Writes the C header for the C ABI in src/ffi*.rs to the path in RANT_CONFIG_HEADER,
// which the CMake build sets. A plain cargo build or test leaves it unset and skips it.
fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-env-changed=RANT_CONFIG_HEADER");
    let Ok(out) = std::env::var("RANT_CONFIG_HEADER") else {
        return;
    };
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let config = cbindgen::Config::from_file(format!("{dir}/cbindgen.toml")).expect("cbindgen.toml");
    cbindgen::Builder::new()
        .with_crate(&dir)
        .with_config(config)
        .generate()
        .expect("generating rant_config.h")
        .write_to_file(out);
}
