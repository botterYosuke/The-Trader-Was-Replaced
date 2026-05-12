use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=python/proto/engine.proto");

    // Use protoc-bin-vendored to ensure build portability
    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    
    // SAFETY: We are in build.rs and this is single-threaded.
    unsafe {
        env::set_var("PROTOC", protoc_path);
    }
    
    tonic_build::compile_protos("python/proto/engine.proto")?;
    Ok(())
}
