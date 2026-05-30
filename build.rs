use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=python/proto/engine.proto");

    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    // SAFETY: single-threaded build.rs context
    unsafe {
        env::set_var("PROTOC", protoc_path);
    }

    prost_build::compile_protos(
        &["proto/engine.proto"],
        &["proto"],
    )?;

    Ok(())
}
