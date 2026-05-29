//! Windows での python3.dll DLL 検索パス設定（issue #66）。
//!
//! `pyo3 = { features = ["auto-initialize"] }` を有効にすると backcast.exe が
//! `python3.dll`（abi3）にリンクする。Windows では `python3.dll` は uv 管理の
//! base Python ディレクトリにあり、venv の `Scripts/` には存在しない。
//! PATH にそのディレクトリがないと OS ローダ段階に `STATUS_DLL_NOT_FOUND (0xC0000135)` で
//! 無言即死する（issue #66）。

use std::path::PathBuf;

/// `python3.dll` を含むディレクトリを返す。
///
/// 以下の順で探索する:
/// 1. 環境変数 `PYTHON_DLL_DIR` が設定されていればそのパスを返す
/// 2. （将来）`where python3.dll` などで自動検出
pub fn find_python_dll_dir() -> Option<PathBuf> {
    std::env::var("PYTHON_DLL_DIR").ok().map(PathBuf::from)
}

/// Python DLL ディレクトリを Windows の DLL 検索パスに追加する。
///
/// `find_python_dll_dir()` が `Some(dir)` を返したとき、`SetDllDirectoryW(dir)` を呼び
/// OS ローダが `python3.dll` を発見できるようにする。
/// `None` のときは no-op。
///
/// # Safety
/// `SetDllDirectoryW` はプロセス全体の DLL 検索パスを変更する。
/// `App::new()` より前、スレッド生成前に一度だけ呼ぶこと。
#[cfg(target_os = "windows")]
pub fn setup_python_dll_search_path() {
    use std::os::windows::ffi::OsStrExt as _;

    unsafe extern "system" {
        #[link_name = "SetDllDirectoryW"]
        fn set_dll_directory_w(lp_path_name: *const u16) -> i32;
    }

    if let Some(dir) = find_python_dll_dir() {
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let ok = unsafe { set_dll_directory_w(wide.as_ptr()) };
        if ok == 0 {
            bevy::log::warn!("[python_env] SetDllDirectoryW({:?}) failed", dir);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn setup_python_dll_search_path() {
    // non-Windows では no-op
}

#[cfg(test)]
mod tests {
    use super::find_python_dll_dir;
    use serial_test::serial;

    /// P12: `PYTHON_DLL_DIR` 環境変数が設定されているとき `find_python_dll_dir()` は `Some` を返すこと。
    #[test]
    #[serial]
    fn python_dll_dir_returns_some_when_env_set() {
        unsafe {
            std::env::set_var("PYTHON_DLL_DIR", r"C:\fake\python");
        }
        let result = find_python_dll_dir();
        unsafe {
            std::env::remove_var("PYTHON_DLL_DIR");
        }
        assert!(result.is_some(), "find_python_dll_dir() must return Some when PYTHON_DLL_DIR is set (issue #66)");
    }
}
