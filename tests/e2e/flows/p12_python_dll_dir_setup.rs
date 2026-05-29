//! P12（kind:integration + kind:manual-gate）python3.dll 不在で 0xC0000135 でクラッシュしないこと
//!
//! `PYTHON_DLL_DIR` 未設定かつ `python3.dll` が PATH にない状態では OS ローダが
//! 0xC0000135 でクラッシュする。`setup_python_dll_search_path()` は `SetDllDirectoryW`
//! を呼ぶが静的リンク解決には間に合わないため、回避策は `PYTHON_DLL_DIR` env var か
//! PATH への追加である。
//!
//! `find_python_dll_dir()` の unit test（`PYTHON_DLL_DIR` env を読んで `Some` を返す）は
//! `src/python_env.rs::tests::python_dll_dir_returns_some_when_env_set` にある。
//!
//! 実ウィンドウ起動検証: Python が PATH にない環境で `backcast.exe` を実行し、
//! exit code が `0xC0000135` にならないことを手動確認する（kind:manual-gate）。
//! issue #66。
