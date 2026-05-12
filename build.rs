use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tell cargo to rerun if proto changes
    println!("cargo:rerun-if-changed=python/proto/engine.proto");

    let protoc_found = if env::var("PROTOC").is_ok() {
        true
    } else {
        match Command::new("protoc").arg("--version").output() {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    };

    if !protoc_found {
        let out_dir = PathBuf::from(env::var("OUT_DIR")?);
        let wrapper_path = out_dir.join("protoc_wrapper.bat");
        
        let project_root = env::current_dir()?;
        let venv_python = project_root.join(".venv").join("Scripts").join("python.exe");
        
        let python_cmd = if venv_python.exists() {
            venv_python.to_str().unwrap().to_string()
        } else {
            "python".to_string()
        };

        let wrapper_content = format!(
            "@echo off\n\"{}\" -m grpc_tools.protoc %*\n",
            python_cmd
        );
        
        fs::write(&wrapper_path, &wrapper_content)?;
        
        // SAFETY: build.rs is single-threaded.
        unsafe {
            env::set_var("PROTOC", &wrapper_path);
        }
    }
    
    tonic_build::compile_protos("python/proto/engine.proto")?;
    Ok(())
}
