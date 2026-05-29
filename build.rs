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

    // NOTE: /DELAYLOAD:python3.dll は MSVC では使えない。
    // python3.dll はデータシンボル (Py_None 等) を持つため、MSVC の delay-load
    // はリンク時 LNK1194 で失敗する。
    // DLL の検索パス設定は実行時に setup_python_dll_search_path() が担う。
    Ok(())
}
