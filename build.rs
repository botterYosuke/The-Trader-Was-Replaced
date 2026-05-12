use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let wrapper_path = out_dir.join("protoc_wrapper.bat");
    
    let project_root = env::current_dir()?;
    let python_path = project_root.join(".venv").join("Scripts").join("python.exe");
    
    let wrapper_content = format!(
        "@echo off\n\"{}\" -m grpc_tools.protoc %*\n",
        python_path.display()
    );
    
    fs::write(&wrapper_path, wrapper_content)?;
    
    // SAFETY: We are in build.rs and this is single-threaded.
    unsafe {
        env::set_var("PROTOC", &wrapper_path);
    }
    
    tonic_build::compile_protos("python/proto/engine.proto")?;
    Ok(())
}
