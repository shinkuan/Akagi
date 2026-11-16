use std::{env, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    // Windows protoc can misdecode non-ASCII absolute paths. Cargo runs this
    // script at the crate root, so prefer a relative descriptor output path.
    let crate_dir = env::current_dir().expect("crate directory unavailable");
    let descriptor_dir = out_dir.strip_prefix(&crate_dir).unwrap_or(&out_dir);

    prost_build::Config::new()
        .type_attribute(".", "#[allow(dead_code)]")
        .file_descriptor_set_path(descriptor_dir.join("liqi_desc.bin"))
        .compile_protos(
            &["src/bridge/majsoul/proto/liqi.proto"],
            &["src/bridge/majsoul/proto/"],
        )
        .expect("failed to compile liqi.proto");

    println!("cargo:rerun-if-changed=src/bridge/majsoul/proto/liqi.proto");
    println!("cargo:rerun-if-changed=build.rs");

    // Surface the build target triple to runtime code so it can pick the
    // right bundled python-build-standalone / uv binary out of the Tauri
    // resource dir. Cargo only exposes this via the `TARGET` env var at
    // build time; we forward it as `TARGET_TRIPLE` for the binary.
    let target = env::var("TARGET").expect("TARGET not set by cargo");
    println!("cargo:rustc-env=TARGET_TRIPLE={target}");

    tauri_build::build();
}
