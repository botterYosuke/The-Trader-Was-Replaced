# Venue メニュー 5 項目を機能させる (Phase 8 §3.2.1 Step 4 完成 + factory plumbing + UI gating)

## Context

`src/ui/menu_bar.rs:200-204` の Venue メニュー (Connect Tachibana Demo/Prod、Connect kabuStation Verify/Prod、Disconnect) は **UI→gRPC→backend 配線まで完成しているが、現状ほぼ常に失敗する**。原因は以下 2 点:

1. UI 側 (`src/ui/menu_bar.rs:324`) が `credentials_source: "prompt"` を**ハードコード送信**しているが、Python adapter (`python/engine/exchanges/tachibana.py:66-69`, `kabusapi.py:50-51`) は `prompt` モードを `NotImplementedError` / `ValueError` で reject する。実装済みは `"env"` モードのみ。
2. factory (`python/engine/live/live_adapter_factory.py:26-29`) が `lambda: TachibanaAdapter()` / `lambda: KabuStationAdapter()` を返すため、UI が送る `environment_hint` ("demo"/"prod"/"verify") が adapter コンストラクタの `environment` 引数に**届かない**。Tachibana は常に "demo"、kabu は常に "verify" に固定される。

仕様 (`docs/plan/Phase 8 - Live Venue and Market Data.md`):
- **§3.2.1 (line 847)** — Python 側 tkinter サブプロセスで認証ダイアログを出す方針。骨格 (`python/engine/live/login_dialog_runner.py`) は完了済み、🟡 「実 tkinter UI、`tachibana_login_flow.py` / `kabusapi_login_flow.py`、`_build_mode.py` の debug 判定定数」が未実装。
- **§3.5.6 (line 985)** — 「Rust 側にログインフォームを実装しない」。クリック後は Python ログインウィンドウがフォーカスを取る。
- **§7 ADR (line 1534-)** — 平文資格情報を **Rust → Python (gRPC)** に乗せない。Python subprocess → Python backend は同一信頼境界。
- **1 backend = 1 venue** (server_grpc.py D26 / line 898-914)。`--live-venue TACHIBANA` で起動中に kabu を選ぶと `VENUE_MISMATCH` で reject される (仕様)。UI 側で接続中の反対 venue ログインを disable して明示する。

本タスクは Phase 8 §3.2.1 Step 4 残作業 (= tkinter 本実装 + subprocess spawn + venue flow) に、factory `environment_hint` plumbing と Rust UI 側の disable ロジックを加えて、5 メニューが端から端まで動く状態を作る。

---

## 設計契約 (実装前に固める)

レビュー指摘 (Critical 1/2) で「subprocess 認証結果を backend adapter に届ける経路」が空白だったため、本セクションで Step 1 より前に契約を確定する。

### C-1. credentials_source の語彙

| 値 | 利用者 | 意味 |
|----|--------|------|
| `env` | デバッグ起動時の高速パス | `DEV_*` env を adapter が直読し、subprocess を spawn しない。 |
| `session_cache` | **Tachibana 専用** | adapter が `tachibana_file_store.load_session()` でセッション dict を読む。 |
| `prompt_result` | **kabu 専用 (新規)** | subprocess が pipe FD 経由で渡してきた token を adapter が in-memory 保存する。`prompt` ではなく `prompt_result` という別ソースとして扱う (現行 `prompt` の `NotImplementedError` を壊さない)。 |
| `prompt` | UI からの初期トリガー (gRPC payload) | backend が受け取り、Tachibana なら subprocess 後 `session_cache` に内部詰め替え、kabu なら subprocess 後 `prompt_result` に詰め替えて adapter.login を呼ぶ。 |

要点: **`prompt` は gRPC 表層の語彙、`session_cache` / `prompt_result` は adapter 表層の語彙**。両者を内部で詰め替えるのは backend (server_grpc) の責務。

### C-2. subprocess ↔ backend IPC (cross-platform)

stdout は **NDJSON の結果メッセージ専用**。token / password は乗せない。

機密データの受け渡しは、**Windows でも動く**よう以下に統一する (`subprocess.pass_fds` は POSIX 限定で、kabuStation 本体は Windows 必須のためその経路を選べない / kabusapi skill S5):

1. parent (backend) が `tempfile.mkstemp(prefix="ttwr_cred_", suffix=".json")` で書き込み権限付き fd を取得し、即 `os.close(fd)`。生成された path を `--cred-path <ABS_PATH>` で subprocess に渡す。
2. parent は file mode を `0o600` に `os.chmod` (POSIX のみ。Windows では既定 ACL = カレントユーザのみ書込 + Administrator 読書、で許容)。
3. subprocess は **成功時のみ** その path に `json.dumps({"token": "<bearer>"})` を **`os.O_WRONLY | os.O_TRUNC`** で 1 行書き込み (既存内容を必ず破棄、追記しない)。失敗 / Cancel / Timeout 時は file を一切 touch しない。
4. parent は `proc.wait()` 後、stdout NDJSON の `success=True` を確認してから cred-path を `open(...).read()` → `os.unlink(cred_path)`。`finally` で常に `os.unlink(cred_path, missing_ok=True)`。
5. cred-path の中身は **token 1 個のみ**。複数値が必要になる将来は同じ JSON object を拡張する。

`pass_fds` / `os.pipe()` は使わない。理由:
- `subprocess.Popen(..., pass_fds=...)` は CPython で POSIX のみサポート (Windows では `ValueError`)。
- Windows のハンドル継承は `STARTUPINFO.lpAttributeList` + `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` が必要で実装が重い。
- tempfile 方式は OS 中立、テストで `tmp_path` fixture をそのまま使える、`_teardown` 経路で取りこぼしを `unlink(missing_ok=True)` で吸収できる、の三利がある。

- Tachibana 経路: subprocess が `_auth_login()` 成功後、`tachibana_file_store.save_session({"url_request": ..., "url_master": ..., "url_price": ..., "url_event": ..., "url_event_ws": ..., "issued_jst_date": ...})` でディスク保存。**cred-path には何も書かない** (session URL はディスク経由、`--cred-path` 引数自体を渡さない)。
- kabu 経路: subprocess が `fetch_token()` 成功後、cred-path に `{"token": "<bearer>"}` を書く。kabuStation token はメモリ常駐のみ (skill S4) なので backend は読み込み後即 `os.unlink`。

stderr は logging / 例外 trace 用。

### C-3. adapter `is_logged_in` 仕様

両 adapter に property を追加する。`server_grpc.py:932` の既定 `True` フォールバックが今は login をスキップする原因なので、必ず実装する。

- `TachibanaAdapter.is_logged_in` → `self._session is not None`
- `KabuStationAdapter.is_logged_in` → `self._token is not None`

### C-4. VenueLogin の I/O 時間予算

subprocess 内で人間がパスワードを打つため、**login 専用 timeout** を新設する (env `LIVE_LOGIN_TIMEOUT_S`、default `180` = 3 分)。既存 `_live_timeout_s` は他 RPC 用に温存。`asyncio.wait_for` で wrap し、超過時は `proc.kill()` → `LOGIN_TIMEOUT` 返却。gRPC handler thread は grpc.server の threadpool worker なので、その thread を blocking しても他 RPC は止まらないが、UI 側でも独立に進捗を取れるよう将来 `VenueLoginStream` を検討 (本タスク非対象、TODO コメントだけ残す)。

---

## 実装ステップ

### Step 0: adapter `is_logged_in` 追加 + Tachibana `session_cache` 実装 (依存 0、最初に着手)

**`python/engine/exchanges/tachibana.py`:**

```python
@property
def is_logged_in(self) -> bool:
    return self._session is not None

async def login(self, creds: VenueCredentials) -> None:
    source = creds.credentials_source
    if source == "session_cache":
        from engine.exchanges.tachibana_file_store import load_session, is_session_valid_for_today
        data = load_session()
        if data is None:
            raise ValueError("SESSION_CACHE_MISSING")
        if not is_session_valid_for_today(data):
            raise ValueError("SESSION_CACHE_EXPIRED")
        self._session = TachibanaSession(
            url_request=RequestUrl(data["url_request"]),
            url_master=MasterUrl(data["url_master"]),
            url_price=PriceUrl(data["url_price"]),
            url_event=EventUrl(data["url_event"]),
            url_event_ws=data["url_event_ws"],
            zyoutoeki_kazei_c=data.get("zyoutoeki_kazei_c", ""),
        )
        # Fix High-2: subprocess の login で消費した p_no を backend counter に反映し、
        # fetch_instruments が同じ p_no を再利用しないようにする。
        last_p_no = data.get("last_p_no")
        if isinstance(last_p_no, int) and last_p_no > self._p_no_counter.peek():
            self._p_no_counter._value = last_p_no
        return
    if source == "prompt":
        raise NotImplementedError(...)  # 現状維持 (backend が prompt→session_cache 詰め替えするため adapter には来ない想定)
    ...
```

**`python/engine/exchanges/kabusapi.py`:**

```python
@property
def is_logged_in(self) -> bool:
    return self._token is not None

async def login(self, creds: VenueCredentials) -> None:
    if self._client.is_closed:
        self._client = httpx.AsyncClient()
    source = creds.credentials_source
    if source == "prompt_result":
        # backend が subprocess から pipe FD 経由で受け取った token を
        # VenueCredentials.token (新規フィールド) に詰めて渡す。
        if not creds.token:
            raise ValueError("PROMPT_RESULT_MISSING_TOKEN")
        self._token = creds.token
        return
    if source == "session_cache":
        raise ValueError("UNSUPPORTED_FOR_VENUE: kabu does not support session_cache")
    if source == "prompt":
        raise NotImplementedError(...)  # 現状維持
    ...  # 既存 env 経路
```

**`python/engine/live/adapter.py` の `VenueCredentials` を 2 箇所同時に変更**:

1. `credentials_source` の Literal を `Literal["prompt", "session_cache", "env", "prompt_result"]` に拡張する (設計契約 C-1 の `prompt_result` を pydantic レイヤで受理させる)。`"prompt_result"` を Step 5 で backend が construct するため、Literal 拡張が抜けると `ValidationError` で経路が壊れる。
2. `token: Optional[str] = None` を追加 (kabu prompt_result 専用、Tachibana 経路では未使用)。`from typing import Optional` の import 確認も併せて行う (現状 `Annotated, AsyncIterator, Literal, ...` のみで `Optional` は import 済かどうか line 12 を要確認)。

`frozen=True` を保つので、construct 時にしか token をセットできない点も維持。

テスト (`python/tests/exchanges/test_tachibana_adapter.py`, `test_kabusapi_adapter.py` 既存ファイルに追加):
- `is_logged_in` が `False`/`True` を正しく返すこと (session/token を直接代入してチェック)。
- Tachibana session_cache 経路: `save_session` で書いた dict (`zyoutoeki_kazei_c` と `last_p_no` を含む) を `login()` で復元できる (`tachibana_file_store` を temp dir で monkeypatch)。復元後に `adapter._session.zyoutoeki_kazei_c` が保存値と一致し、`adapter._p_no_counter.peek()` が `last_p_no` 以上であることを assert する (counter advance の検証)。
- Tachibana session_cache 経路: `last_p_no` が session dict に存在しない (旧フォーマット後方互換) 場合、counter advance をスキップして正常復元できること。
- Tachibana session_cache 経路: `SESSION_CACHE_MISSING` / `SESSION_CACHE_EXPIRED` を raise する。
- kabu prompt_result 経路: token あり → 成功、token なし → `PROMPT_RESULT_MISSING_TOKEN`。
- **`VenueCredentials(credentials_source="prompt_result", token="x")` が pydantic validation を通る** ことの smoke test を `python/tests/live/test_adapter.py` (既存があれば拡張、なければ新規) に追加。同様に `credentials_source="prompt_result", token=None` を作って `PROMPT_RESULT_MISSING_TOKEN` が adapter 側で raise されることを kabu test で検証 (model 側では `None` 許容のまま、validation は adapter 責務)。

### Step 1: `python/engine/live/_build_mode.py` (新規) + release pipeline freeze script

ビルド時に書き換える単一定数ファイル。

- 内容: `IS_DEBUG_BUILD = True` (default)。
- 配布バイナリ (release wheel) では **release pipeline 側のスクリプト** がこれを `False` に書き換える。`__debug__` や env var は使わない (§7 ADR / line 1564)。
- **build hook は使わない** — `pyproject.toml` の build backend は `setuptools.build_meta` で、`[tool.hatch.build.hooks.custom]` を足しても発火しない。代わりに `tools/freeze_build_mode.py` を新規追加し、release pipeline (CI) が wheel 作成前に `python tools/freeze_build_mode.py --release` を呼ぶ運用にする。スクリプトは単純な文字列置換 (`IS_DEBUG_BUILD = True` → `... = False`)。
- ユニットテスト (`python/tests/live/test_build_mode.py`):
  - `IS_DEBUG_BUILD` が import できる smoke test。
  - `tools/freeze_build_mode.py` を tmp_path で実行し、フラグが反転することを確認。

### Step 2: `python/engine/exchanges/tachibana_login_flow.py` (新規) — Tachibana 固有 tkinter フォーム

**設計分割 (headless テスト対応)**: ロジックを 2 ファイルに割る。

- `tachibana_login_form_state.py` — presenter / pure logic。tkinter import なし。
  - `@dataclass(frozen=True) class FormInit` (env_hint, allow_prod, is_debug_build, dev_user_id, dev_password, dev_demo)。
  - `build_form_init(env_hint, env_dict, is_debug_build) -> FormInit` — env 読みと debug build 判定をここで完結させる純粋関数。
  - `validate_submission(user_id, password, mode) -> Optional[str]` — 必須チェックを返す (空欄 → `"EMPTY_FIELDS"`)。
  - エラーコード文字列定数: `AUTH_FAILED` / `NETWORK_ERROR` / `SERVICE_OUT_OF_HOURS` / `USER_CANCELLED` / `EMPTY_FIELDS`。
- `tachibana_login_flow.py` — tkinter view + asyncio ブリッジ。
  - 入口 `run_dialog(env_hint: str) -> dict`。
  - 上記 `FormInit` を組み立てて `tk.Tk()` を構築。
  - フィールド: ユーザー ID `Entry`、パスワード `Entry(show="*")`、demo/prod の `Radiobutton`、OK/Cancel ボタン。`init.allow_prod=False` で prod radio `state="disabled"` (§3.2.1 line 863)。`init.is_debug_build=True` 時のみ `dev_*` を `Entry.insert()` / radio 初期値に反映 (§3.2 line 781-785)。release では env を一切読まない。
  - **async ブリッジ**: OK callback は同期だが `engine.exchanges.tachibana_auth.login(...)` は async。callback では:
    1. `ok_btn.config(state="disabled")` / Cancel も `disabled` / 「Authenticating...」ラベル表示。
    2. `result_holder: dict = {}` を closure capture して **新規スレッド** で以下を回し、終了後に `root.after(0, lambda: _on_auth_done(...))` で UI スレッドへ復帰。`asyncio.run` を直接 callback で呼ぶと `httpx.AsyncClient` の close warning + tk mainloop 停止のリスクがあるためスレッド分離する。

       ```python
       # PNoCounter は run_dialog スコープで生成し、ここで参照する (closure capture)。
       # login() は keyword-only 必須引数 p_no_counter を要求するため省略不可。
       from engine.exchanges.tachibana_auth import PNoCounter
       p_no_counter = PNoCounter()   # run_dialog スコープで 1 回だけ生成

       def _run_auth():
           try:
               session = asyncio.run(
                   tachibana_auth.login(
                       user_id, password,
                       is_demo=(selected_mode == "demo"),
                       p_no_counter=p_no_counter,   # ← 必須。省略すると TypeError
                   )
               )
               result_holder["v"] = ("ok", session)
           except Exception as exc:
               result_holder["v"] = ("err", exc)
           root.after(0, _on_auth_done)

       threading.Thread(target=_run_auth, daemon=True).start()
       ```

    3. `_on_auth_done` 内で `save_session(...)` (成功時: `p_no_counter.peek()` を含む) / error code セット (失敗時) → `root.destroy()`。
- OK 成功時 save_session 入力 dict:

  ```python
  # p_no_counter は run_dialog スコープで生成し、login() に渡してから peek() する。
  save_session({
      "url_request": str(session.url_request),
      "url_master": str(session.url_master),
      "url_price": str(session.url_price),
      "url_event": str(session.url_event),
      "url_event_ws": session.url_event_ws,
      "zyoutoeki_kazei_c": session.zyoutoeki_kazei_c,  # Fix Critical: 必須フィールド
      "last_p_no": p_no_counter.peek(),               # Fix High-2: backend counter 継続用
      "issued_jst_date": datetime.now(ZoneInfo("Asia/Tokyo")).date().isoformat(),
  })
  ```

  仮想 URL と `zyoutoeki_kazei_c` のみ。ID/PW はディスクに書かない (skill S3)。
  `p_no_counter` は上記 async ブリッジのコードスケッチ通り `run_dialog` スコープで生成し、`_run_auth` / `_on_auth_done` が closure 経由で参照する。`save_session` 呼び出し時点で `peek()` して login で消費した最終値を記録する。
- 戻り値: `{"success": True, "error_code": ""}` または `{"success": False, "error_code": "AUTH_FAILED"|"NETWORK_ERROR"|"SERVICE_OUT_OF_HOURS"|"USER_CANCELLED"|"EMPTY_FIELDS"}` (既存 `tachibana_auth` の例外種別から導出)。
- 第二暗証番号は本タスク非対象 (skill F-H5 / §7 ADR line 1547)。

テスト分割 (headless CI 対策 — display 不要):
- `python/tests/exchanges/test_tachibana_login_form_state.py` (display 不要、CI でも常時走らせる)
  - `build_form_init` が `TACHIBANA_ALLOW_PROD=1` 立ちで `allow_prod=True` を返す。
  - `build_form_init` が `is_debug_build=False` のとき `dev_user_id`/`dev_password`/`dev_demo` が全て `None` (= release では env を読まない)。
  - `build_form_init(env_hint="prod", allow_prod=True)` が `initial_mode="prod"` を返す / `allow_prod=False` のとき `initial_mode="demo"` にダウングレード。
  - `validate_submission("", "x", "demo")` が `"EMPTY_FIELDS"`。
- `python/tests/exchanges/test_tachibana_login_flow.py` (tkinter 必須、display gating あり)
  - ファイル冒頭で `pytest.importorskip("tkinter")` + `_HAS_DISPLAY = bool(os.environ.get("DISPLAY")) or sys.platform == "darwin" or sys.platform == "win32"; pytestmark = pytest.mark.skipif(not _HAS_DISPLAY, reason="no display available")` を置く。
  - widget 構築後に prod radio が `state="disabled"` / `"normal"` を `cget("state")` で assert。
  - `tachibana_auth.login` を monkeypatch (`AsyncMock`) で wrap → callback 経由で `save_session` が呼ばれることを `unittest.mock.patch` で観測。
  - **asyncio ブリッジテスト**: monkeypatched async login が `asyncio.sleep(0)` 1 回挟んでも UI が dead-lock しない (`root.after(50, root.destroy)` をタイマーで仕掛けて mainloop 復帰を検証)。

### Step 3: `python/engine/exchanges/kabusapi_login_flow.py` (新規) — kabu 固有 tkinter フォーム

**設計分割 (Step 2 と同パターン)**: ロジックを 2 ファイルに割る。

- `kabusapi_login_form_state.py` — presenter / pure logic。tkinter import なし。
  - `@dataclass(frozen=True) class FormInit` (env_hint, allow_prod, is_debug_build, dev_api_password, station_port)。
  - `build_form_init(env_hint, env_dict, is_debug_build) -> FormInit` (release では `dev_api_password=None`)。
  - `probe_station(host="127.0.0.1", port=int) -> bool` — `socket.create_connection(..., timeout=0.5)` を try/except して bool を返す純粋関数。本物 socket を叩くので CI では `monkeypatch.setattr(socket, "create_connection", ...)` で隔離。
- `kabusapi_login_flow.py` — tkinter view。
  - 入口 `run_dialog(env_hint: str, cred_path: str) -> dict` (Critical 2 対応で **`cred_fd: int` → `cred_path: str`** に変更)。
  - フィールド: API パスワード `Entry(show="*")`、verify/prod の `Radiobutton`、kabuStation 本体の listening ポート表示 (read-only Label)、[再確認] ボタン、OK/Cancel。
  - prod ラジオは `init.allow_prod=False` で disabled (§3.2.1 line 864)。
  - 起動時に `probe_station(port=18081 if env=="verify" else 18080)` で本体検出。未起動なら `KABU_STATION_NOT_RUNNING` メッセージ表示 + [再確認] ボタン (本体起動後リトライ)。
  - `init.is_debug_build=True` 時のみ `dev_api_password` を prefill。
  - **async ブリッジ**: Step 2 と同じく OK callback 内で OK/Cancel disable + 「Authenticating...」表示 → 新規スレッドで `asyncio.run(kabusapi_auth.fetch_token(api_password, env=...))` → `root.after(0, ...)` で UI 復帰。
  - 成功時の token 書き出し:
    ```python
    fd = os.open(cred_path, os.O_WRONLY | os.O_TRUNC)
    try:
        os.write(fd, json.dumps({"token": token}).encode("utf-8"))
    finally:
        os.close(fd)
    ```
    `O_TRUNC` で既存内容を必ず消す。`json.dumps` の末尾改行は付けない (parent は `read()` 全体を `json.loads` する)。
- 戻り値 (stdout NDJSON): `{"success": True, "error_code": ""}` または `KABU_STATION_NOT_RUNNING` / `KABU_API_DISABLED` (`4001003`) / `KABU_TOKEN_EXPIRED` (`4001005`) / `AUTH_FAILED` / `USER_CANCELLED` / `EMPTY_FIELDS`。**token は stdout に乗せない**。

テスト分割:
- `python/tests/exchanges/test_kabusapi_login_form_state.py` (display 不要、CI で常時実行)
  - `build_form_init` の env / allow_prod / debug 判定マトリクス (Step 2 と同形)。
  - `probe_station` を `monkeypatch.setattr(socket, "create_connection", raises=ConnectionRefusedError)` で False、成功 mock で True。
- `python/tests/exchanges/test_kabusapi_login_flow.py` (tkinter 必須、display gating あり、Step 2 と同じ `pytest.importorskip` + `skipif`)
  - prod radio disabled when `KABU_ALLOW_PROD` 不在。
  - `KABU_STATION_NOT_RUNNING` 表示 (`probe_station` を monkeypatch で False)。
  - debug build prefill。
  - `fetch_token` monkeypatch (`AsyncMock`) で成功 → tmp_path の `cred_path` に `{"token":"..."}` が書かれること、失敗 → cred_path は **0 バイト** のまま (touch しない) で stdout が `AUTH_FAILED`。

### Step 4: `python/engine/live/login_dialog_runner.py` 本実装 (既存 line 71-73 置換)

既存スケルトンを以下に置換 (C-2 改訂で `--cred-fd` → `--cred-path` に変更):

```python
parser.add_argument("--venue", required=True)
parser.add_argument("--env", required=True)  # demo|prod|verify
parser.add_argument("--cred-path", type=str, default="")
...
if ns.venue == "tachibana":
    from engine.exchanges.tachibana_login_flow import run_dialog
    result = run_dialog(env_hint=ns.env)
elif ns.venue == "kabu":
    from engine.exchanges.kabusapi_login_flow import run_dialog
    if not ns.cred_path:
        emit(_result(False, "MISSING_CRED_PATH"))
        return 0
    result = run_dialog(env_hint=ns.env, cred_path=ns.cred_path)
emit({"type": "result", **result})
return 0
```

**`--env` を required にする** (現行は default=None で `INVALID_ENV` を返す挙動。backend は常に有効な env を渡せるので、空文字を許容する必要なし)。

`VALID_ENVS` チェックも `("demo", "prod", "verify")` のままで OK だが、venue ごとに enforce する (`tachibana` で `verify` は弾く、`kabu` で `demo` は弾く):

```python
_ENV_PER_VENUE = {"tachibana": {"demo", "prod"}, "kabu": {"verify", "prod"}}
if ns.env not in _ENV_PER_VENUE[ns.venue]:
    emit(_result(False, "INVALID_ENV"))
    return 0
```

token 等の機密値は **stdout に乗せない** (§3.2.1 line 856-860)。`error_code` のみ伝播。NDJSON は既存規約 (`{"type":"result","success":bool,"error_code":str}`) に従う。

テスト (`python/tests/live/test_login_dialog_runner.py` を既存があれば拡張、なければ新規):
- `--venue=kabu --cred-path=""` で `MISSING_CRED_PATH` (引数未指定相当)。
- `--venue=tachibana --env=verify` で `INVALID_ENV` (venue ごとの enforce)。
- `try_create_tk` 失敗で `NO_DISPLAY_AVAILABLE` (既存)。
- `run_dialog` を monkeypatch して成功 NDJSON が stdout に出ること。
- `--cred-path` が存在しない path / 書込権限なし path のケースは subprocess 内ではなく **parent (Step 5)** で握る (parent が tempfile を生成する責務)。runner では path 検証しない (簡潔さ優先)。

### Step 5: `python/engine/server_grpc.py::VenueLogin` を subprocess spawn フローに切替

> **実装順序の制約 (Medium)**: Step 5 の `_attempt()` 内は `self._start_live_components(environment_hint=env_hint)` を呼ぶが、このシグネチャ拡張は **Step 6** で行う。同一ファイルへの変更なので **Step 6 → Step 5 の順** で実装すること。逆順だと `TypeError: _start_live_components() got an unexpected keyword argument 'environment_hint'` が発生する。Lane C を担当するエージェント/実装者は 6 を先に commit してから 5 を実装すること。

現在 (line 866-961) の `cred_source == "prompt"` 経路を **subprocess spawn + tempfile IPC** に拡張。**High 1/2 対策で内部関数 `_attempt_login(cred_source, ...)` を切り出し**、prompt 失敗時に NO_DISPLAY_AVAILABLE → debug env への切替を再帰呼出で行う (Python の if/elif/else に fall-through が無いため、状態書換 + コメントでは動かない)。

#### 5a. subprocess 駆動関数

```python
async def _handle_prompt_login(self, venue_id: str, env_hint: str) -> tuple[bool, str, Optional[str]]:
    """Return (success, error_code, token_or_none).

    Cross-platform IPC: kabu の場合のみ tempfile を作って --cred-path で渡す。
    Tachibana は session_cache 経由なので cred-path を渡さない。
    """
    cred_path = ""
    if venue_id.upper() == "KABU":
        fd, cred_path = tempfile.mkstemp(prefix="ttwr_cred_", suffix=".json")
        os.close(fd)
        if os.name == "posix":
            os.chmod(cred_path, 0o600)
    args = [
        sys.executable, "-m", "engine.live.login_dialog_runner",
        "--venue", venue_id.lower(),
        "--env", env_hint,
    ]
    if cred_path:
        args.extend(["--cred-path", cred_path])
    try:
        proc = await asyncio.create_subprocess_exec(
            *args,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        # Fix Medium: stderr を並行 drain するタスクを立てる。
        # stderr=PIPE のまま drain しないと子が大量ログを吐いた場合にカーネルの
        # pipe buffer (~64 KB) が満杯になり、子プロセスが write でブロック →
        # 親の stdout.readline() も永遠に待ち続けるデッドロックになる。
        # drain task は stdout 側の結果が出たあとに await/cancel して stderr を
        # 診断ログに使う。
        stderr_drain = asyncio.ensure_future(proc.stderr.read())
        try:
            line = await asyncio.wait_for(
                proc.stdout.readline(),
                timeout=float(os.environ.get("LIVE_LOGIN_TIMEOUT_S", "180")),
            )
        except asyncio.TimeoutError:
            proc.kill()
            await proc.wait()
            stderr_drain.cancel()
            return False, "LOGIN_TIMEOUT", None
        if not line:
            # プロセスが stdout を閉じずに終了 → stderr_drain の結果を診断に使う
            try:
                stderr_bytes = await asyncio.wait_for(stderr_drain, timeout=5.0)
            except (asyncio.TimeoutError, asyncio.CancelledError):
                stderr_bytes = b""
            logging.error(
                "login_dialog_runner exited without result: %s",
                stderr_bytes.decode("utf-8", errors="replace"),
            )
            await proc.wait()
            return False, "LOGIN_SUBPROCESS_CRASHED", None
        try:
            result = json.loads(line)
        except json.JSONDecodeError:
            proc.kill()
            await proc.wait()
            return False, "LOGIN_INVALID_RESPONSE", None
        if not result.get("success"):
            # Fix High-1: 失敗パスでも proc を清掃する。
            # Windows では kabu の cred_path が子プロセスに open されたまま残ると
            # finally の os.unlink() が PermissionError になるため、先に wait/kill。
            try:
                await asyncio.wait_for(proc.wait(), timeout=5.0)
            except asyncio.TimeoutError:
                proc.kill()
                await proc.wait()
            return False, result.get("error_code") or "AUTH_FAILED", None
        # Fix Medium-4: proc.wait() に timeout を付け、tkinter mainloop 残存で
        # gRPC 予算を消費しないようにする。
        try:
            await asyncio.wait_for(proc.wait(), timeout=10.0)
        except asyncio.TimeoutError:
            proc.kill()
            await proc.wait()
            return False, "LOGIN_TIMEOUT", None
        if proc.returncode != 0:
            return False, result.get("error_code") or "LOGIN_NONZERO_EXIT", None
        token = None
        if cred_path:
            with open(cred_path, "rb") as f:
                blob = f.read()
            if not blob:
                return False, "LOGIN_INVALID_RESPONSE", None
            token = json.loads(blob.decode("utf-8"))["token"]
        return True, "", token
    finally:
        if cred_path:
            try:
                os.unlink(cred_path)
            except (FileNotFoundError, PermissionError):
                # FileNotFoundError: 正常パス (parent が unlink 済み)
                # PermissionError: Windows でまだ子が file を掴んでいるケース
                # (上記 wait_for+kill で大抵防げるが race window のため握る)
                pass
```

#### 5b. VenueLogin 本体の構造化リファクタ

```python
def VenueLogin(self, request, context):
    # ... (token / venue_id / configured_venue チェックまでは既存維持) ...

    # Idempotent: already CONNECTED/SUBSCRIBED → no-op success
    if self.venue_sm is not None and self.venue_sm.current in ("CONNECTED", "SUBSCRIBED"):
        return engine_pb2.VenueLoginResponse(
            success=True, error_code="",
            venue_state=self.venue_sm.current, instruments_loaded=0,
        )

    # Fix High-2: AUTHENTICATING 中は同 venue からの二重起動も拒否する。
    # prompt 経路では subprocess spawn 前に AUTHENTICATING へ遷移するため、
    # 再クリックによる二重起動 → session/token/state 競合を backend で防ぐ。
    if self.venue_sm is not None and self.venue_sm.current == "AUTHENTICATING":
        return engine_pb2.VenueLoginResponse(
            success=False, error_code="ALREADY_AUTHENTICATING",
            venue_state="AUTHENTICATING", instruments_loaded=0,
        )

    cred_source = request.credentials_source or "prompt"
    env_hint = request.environment_hint or None

    # 失敗時に必ず adapter / runner ごと捨てる小ヘルパ。
    def _fail(error_code: str) -> engine_pb2.VenueLoginResponse:
        if self._live_runner is not None or self._live_bridge is not None:
            self._teardown_live_components()
        return engine_pb2.VenueLoginResponse(
            success=False,
            error_code=error_code,
            venue_state=self.venue_sm.current if self.venue_sm else "DISCONNECTED",
            instruments_loaded=0,
        )

    def _attempt(effective_source: str) -> tuple[bool, str]:
        """1 回の login 試行。`(handled, error_code)` を返す。

        handled=True / error_code="" → 成功して関数末尾の success レスポンスに進む。
        handled=True / error_code != "" → 呼び出し側が _fail(error_code) を返す。
        handled=False → 呼び出し側が別 source で再試行 (NO_DISPLAY → env のみで使用)。
        """
        try:
            self._start_live_components(environment_hint=env_hint)
            runner = self._live_runner
            adapter = runner.adapter
            loop = self._ensure_live_loop()

            if effective_source == "prompt":
                # Fix High-3: subprocess spawn 前に AUTHENTICATING へ遷移。
                # UI が別 poll で状態を読んでいる場合に「ダイアログ表示中」が見える。
                # 失敗時は _fail() が teardown + DISCONNECTED へ戻す。
                if self.venue_sm is not None and self.venue_sm.current == "DISCONNECTED":
                    self.venue_sm.transition_to("AUTHENTICATING")

                # Fix Medium-5: effective_env を subprocess と factory で統一して渡す。
                # env_hint が空 / None の場合も runner の --env required=True に対応。
                if venue_id == "TACHIBANA":
                    effective_env = env_hint if env_hint in ("demo", "prod") else "demo"
                else:
                    effective_env = env_hint if env_hint in ("verify", "prod") else "verify"

                fut = asyncio.run_coroutine_threadsafe(
                    self._handle_prompt_login(venue_id, effective_env),
                    loop,
                )
                success, ec, token = fut.result(
                    timeout=float(os.environ.get("LIVE_LOGIN_TIMEOUT_S", "180")) + 10,
                )
                if not success:
                    if ec == "NO_DISPLAY_AVAILABLE":
                        from engine.live._build_mode import IS_DEBUG_BUILD
                        if IS_DEBUG_BUILD:
                            return False, ec   # caller retries with "env"
                    return True, ec
                # prompt 成功 → adapter 表層の語彙に詰め替えて adapter.login
                from engine.live.adapter import VenueCredentials
                if venue_id == "TACHIBANA":
                    adapter_creds = VenueCredentials(
                        credentials_source="session_cache",
                        environment_hint=effective_env,
                    )
                else:  # KABU
                    adapter_creds = VenueCredentials(
                        credentials_source="prompt_result",
                        environment_hint=effective_env,
                        token=token,
                    )
            else:
                # env / session_cache の直接指定経路
                from engine.live.adapter import VenueCredentials
                adapter_creds = VenueCredentials(
                    credentials_source=effective_source,
                    environment_hint=env_hint,
                )

            if not getattr(adapter, "is_logged_in", True):
                login_fut = asyncio.run_coroutine_threadsafe(
                    adapter.login(adapter_creds), loop,
                )
                login_fut.result(timeout=self._live_timeout_s)

            # prompt 経路では AUTHENTICATING は spawn 前に遷移済み。
            # env / session_cache 経路はここで遷移する。
            if self.venue_sm is not None and self.venue_sm.current == "DISCONNECTED":
                self.venue_sm.transition_to("AUTHENTICATING")
            if self.venue_sm is not None and self.venue_sm.current == "AUTHENTICATING":
                self.venue_sm.transition_to("CONNECTED")
            return True, ""
        except Exception as exc:
            logging.exception("VenueLogin attempt failed (source=%s): %s", effective_source, exc)
            return True, "VENUE_LOGIN_FAILED"

    handled, error_code = _attempt(cred_source)
    if not handled and cred_source == "prompt":
        # NO_DISPLAY_AVAILABLE in debug build → tear down stale runner first,
        # then retry once with env. release build はここに来ない (handled=True で抜けている)。
        if self._live_runner is not None or self._live_bridge is not None:
            self._teardown_live_components()
        handled, error_code = _attempt("env")

    if error_code:
        return _fail(error_code)

    return engine_pb2.VenueLoginResponse(
        success=True,
        error_code="",
        venue_state=self.venue_sm.current if self.venue_sm else "CONNECTED",
        instruments_loaded=0,
    )
```

主要差分:

- **High 1**: 失敗パスは全て `_fail(...)` 経由で必ず `_teardown_live_components()` を呼ぶ。`_live_runner` / `environment_hint` が前回試行のまま残らない。NO_DISPLAY → env retry の前にも明示的に teardown する (次の `_start_live_components` で新 `env_hint` で adapter を作り直す)。
- **High 2**: `_attempt` を内部関数として切り出し、`(handled=False, "NO_DISPLAY_AVAILABLE")` を返すパスで呼び出し側が `_attempt("env")` を**実呼出**する。`if cred_source == "prompt":` の else 内で変数だけ書き換えていた構造を廃止。
- `session_cache` (Tachibana のみ、UI が明示送信した場合) / `env` 直接指定は `_attempt(cred_source)` 1 回呼びでカバー。失敗は handled=True で `_fail` 経由。

**TODO(将来)**: VenueLoginStream を proto に足して subprocess 状態を逐次 push できるようにすると、UI 側で「ダイアログ表示中」「token 取得中」を区別できる。本タスクでは同期 fut.result でブロック。

テスト (`python/tests/test_grpc_phase8.py` に追加):
- `test_venue_login_prompt_tachibana_spawns_subprocess_and_transitions_connected`
  - `create_subprocess_exec` を monkeypatch して `{"type":"result","success":true,"error_code":""}` を吐かせ、`tachibana_file_store.save_session` も monkeypatch、adapter の `session_cache` 経路が呼ばれることを assert。
- `test_venue_login_prompt_kabu_writes_token_to_cred_path` (旧 `pipes_token` から改名)
  - kabu subprocess mock が `--cred-path` で渡された tempfile に `{"token": "..."}` を書き、成功 NDJSON を吐く。parent が読み取って adapter.login が `prompt_result` + `token=...` で呼ばれ `_token` が埋まる。**テスト後 tempfile が削除されている** ことも assert (`os.path.exists(...) is False`)。
- `test_venue_login_prompt_kabu_empty_cred_file_returns_login_invalid_response`
  - subprocess が成功 NDJSON を吐くが cred-path が空ファイル (= 規約違反) のケース。parent が `LOGIN_INVALID_RESPONSE` を返し、`_teardown_live_components` が呼ばれていること。
- `test_venue_login_prompt_failure_tears_down_runner` (High 1 検証)
  - `AUTH_FAILED` を返す mock subprocess の後、`self._live_runner is None` と `self._live_bridge is None` を assert。直後の再 Connect (別 env_hint) で新 adapter が `env_hint=...` で生成されることを `_resolve_*_env` の return を捕捉して検証。
- `test_venue_login_prompt_failure_transitions_to_error_with_code` (`AUTH_FAILED`, `USER_CANCELLED`, `KABU_STATION_NOT_RUNNING`)。
- `test_venue_login_prompt_timeout_kills_subprocess_and_returns_login_timeout`。
- `test_venue_login_prompt_crash_returns_login_subprocess_crashed` (proc が stdout 何も出さずに exit)。
- `test_venue_login_prompt_invalid_json_returns_login_invalid_response`。
- `test_venue_login_prompt_no_display_fallbacks_to_env_in_debug_only` (High 2 検証)
  - `IS_DEBUG_BUILD=True` で `_handle_prompt_login` が `NO_DISPLAY_AVAILABLE` を返す mock。`_attempt("env")` が**実際に呼ばれた**ことを `_start_live_components` の call args 履歴で検証 (mock spy で「`_attempt` が 2 回呼ばれ、2 回目の引数が `"env"`」を assert)。`IS_DEBUG_BUILD=False` 版は `_fail("NO_DISPLAY_AVAILABLE")` を返し再試行されないこと、`_teardown_live_components` も呼ばれていることを assert。
- `test_venue_login_prompt_no_display_release_tears_down_runner` (High 1 + release path)
  - release build で NO_DISPLAY 直行する経路でも runner / bridge / venue_sm が DISCONNECTED に戻ること。
- `test_venue_login_already_authenticating_returns_error` (Fix High-2: 二重起動防止)
  - venue_sm を手動で AUTHENTICATING に設定した状態で VenueLogin を呼ぶと `ALREADY_AUTHENTICATING` が返り、subprocess が spawn されないこと。

### Step 6: factory + adapter `_env` plumbing

**`python/engine/live/live_adapter_factory.py` 変更:**

closure を引数を取る形に変更:

```python
def build_live_adapter_factory(venue: str) -> Callable[[Optional[str]], LiveVenueAdapter]:
    if venue == "TACHIBANA":
        return lambda env_hint=None: TachibanaAdapter(environment=_resolve_tachibana_env(env_hint))
    if venue == "KABU":
        return lambda env_hint=None: KabuStationAdapter(environment=_resolve_kabu_env(env_hint))
    if venue == "MOCK":
        from engine.live.mock_adapter import MockVenueAdapter
        return lambda env_hint=None: MockVenueAdapter()
    raise UnknownVenueError(f"unknown venue: {venue!r}")

def _resolve_tachibana_env(hint: Optional[str]) -> str:
    if hint in (None, ""):
        return "demo"
    if hint in ("demo", "prod"):
        return hint
    raise ValueError(f"invalid Tachibana environment_hint: {hint!r}")

def _resolve_kabu_env(hint: Optional[str]) -> str:
    if hint in (None, ""):
        return "verify"
    if hint in ("verify", "prod"):
        return hint
    raise ValueError(f"invalid kabu environment_hint: {hint!r}")
```

**`python/engine/server_grpc.py` 変更:**

`_start_live_components(self, environment_hint: Optional[str] = None)` の signature を追加して `environment_hint` を受け取り、`self._live_adapter_factory(environment_hint)` で呼ぶ。VenueLogin handler で `self._start_live_components(environment_hint=request.environment_hint or None)` を呼ぶ。

**注意点**: adapter は `_start_live_components` 内で 1 回だけ生成され `self._live_runner.adapter` にキャッシュされる。Disconnect→再 Connect で別環境を選んだとき、`_teardown_live_components` が走るので次回 Login で新環境の adapter が再生成される (`server_grpc.py:967-969`)。再 Connect で同 venue・別 env を選ぶケースをテストでカバー。

テスト (`python/tests/live/test_live_adapter_factory.py` 拡張):
- `test_factory_tachibana_demo_returns_demo_adapter`
- `test_factory_tachibana_prod_returns_prod_adapter`
- `test_factory_kabu_verify_returns_verify_adapter`
- `test_factory_kabu_prod_returns_prod_adapter`
- `test_factory_default_environment_when_hint_none`
- `test_factory_invalid_hint_raises`

`test_grpc_phase8.py` の既存 mock 系テストは `lambda env_hint=None: ...` シグネチャに合わせて更新。

### Step 7: Rust UI 側 — ログイン中は他 venue メニューを disable

**`src/trading.rs` に新 helper を追加:**

```rust
/// Returns `true` when the venue is in any state that occupies the slot
/// (Authenticating / Connected / Subscribed). Used by menu_bar gating
/// to disable opposite-venue Connect items.
pub fn is_venue_busy_for_menu(state: VenueState) -> bool {
    matches!(
        state,
        VenueState::Authenticating | VenueState::Connected | VenueState::Subscribed,
    )
}
```

既存 `is_venue_live` (`Connected | Subscribed`) は他箇所で使われているので変更しない。

**`src/ui/menu_bar.rs` 変更:**

新 system `gate_venue_menu_items_system` を `Update` で動かす:

- Query: `(&MenuItem, &mut BackgroundColor, &Children) With<Button>` + `Res<VenueStatusRes>` (リソース名は `VenueStatusRes`、フィールドは `venue_id: Option<String>`、`current_venue_id` ではない)。
- `is_venue_busy_for_menu(status.state)` が true のとき、**反対 venue と同 venue 両方の Connect ボタン**を灰色 (`Color::srgba(0.20, 0.20, 0.20, 0.5)`) + `TextColor` 暗化で disable する。
  - 反対 venue: `status.venue_id.as_deref()` が接続済み venue を示し、押下対象が別 venue の項目なら disable。
  - **同 venue (Fix High-2)**: `AUTHENTICATING` 中に同 venue Connect を再クリックすると backend が `ALREADY_AUTHENTICATING` を返すが、UI でも disable して二重 gRPC 呼び出し自体を防ぐ。`CONNECTED/SUBSCRIBED` 時も同様に disable (視覚的一貫性)。
  - Disconnect は常に通常色。
- 加えて `menu_item_system` の `VenueConnect*` 分岐冒頭で同条件チェック → 早期 `continue` (二重ガード)。`VenueStatusRes` を引数に追加する必要があるので signature 更新。

テスト (`src/ui/menu_bar.rs` の `#[cfg(test)] mod tests` に追加):
- `test_gate_venue_menu_disables_kabu_when_tachibana_authenticating`
- `test_gate_venue_menu_disables_kabu_when_tachibana_connected`
- `test_gate_venue_menu_disables_same_venue_when_authenticating` (Fix High-2: 同 venue も disable)
- `test_gate_venue_menu_enables_all_when_disconnected`
- `test_venue_connect_pressed_during_other_venue_busy_is_ignored`
- `test_venue_connect_same_venue_pressed_during_authenticating_is_ignored` (Fix High-2)

(`bevy-engine` skill / `rust-testing` skill の `App::new()` + `app.update()` パターンを踏襲。`init_resource::<VenueStatusRes>` 漏れに注意。)

### Step 8: UI 側 `credentials_source` の扱い

`src/ui/menu_bar.rs:324` の `credentials_source: "prompt".to_string()` は **変更不要**。プラン §3.5.6 line 985 「`VenueLogin(credentials_source="prompt", environment_hint=...)`」と一致しており、Step 5 の backend 改修で `prompt` 経路が機能するようになる。

debug ビルドユーザーが env 経路で素早く認証したい場合は backend 起動引数 / 環境変数で `DEV_TACHIBANA_*` / `DEV_KABU_API_PASSWORD` を export し、Rust 側で `credentials_source="env"` を送る別エントリは作らない (release との挙動差を build-time 定数 `_build_mode.IS_DEBUG_BUILD` のみで吸収する原則)。`NO_DISPLAY_AVAILABLE` の自動 env fallback は Step 5 で実装済み。

---

## 検証手順

### 自動テスト

```bash
# Python: presenter (display 不要、CI 必須) + adapter + grpc
uv run pytest \
    python/tests/live/ \
    python/tests/exchanges/test_tachibana_adapter.py \
    python/tests/exchanges/test_kabusapi_adapter.py \
    python/tests/exchanges/test_tachibana_login_form_state.py \
    python/tests/exchanges/test_kabusapi_login_form_state.py \
    python/tests/test_grpc_phase8.py -v

# Python: tkinter view (display 必須、ローカル / macOS / Windows のみ)
uv run pytest \
    python/tests/exchanges/test_tachibana_login_flow.py \
    python/tests/exchanges/test_kabusapi_login_flow.py -v

# Rust: menu_bar gating + venue helpers
cargo test --lib menu_bar
cargo test --lib trading::is_venue_busy_for_menu
```

**Windows 検証**: kabu 経路は kabuStation 本体の制約で Windows 必須 (kabusapi skill S5)。**Windows 環境で `pytest python/tests/test_grpc_phase8.py::test_venue_login_prompt_kabu_writes_token_to_cred_path` を必ず実行する**。`tempfile.mkstemp` + `--cred-path` 方式は Windows でも動くことを CI / 手動で確認する (Critical 2)。

### 手動 E2E (`e2e-testing` skill 準拠)

**準備:**
- shell で `export DEV_TACHIBANA_USER_ID=... DEV_TACHIBANA_PASSWORD=... DEV_KABU_API_PASSWORD=...` (debug build で prefill 確認したい場合)。

**Tachibana 検証:**
1. `uv run python -m engine --transport grpc --live-venue TACHIBANA --token <token>` で backend 起動。
2. `cargo run --release --bin backcast` (debug ビルドなら `cargo run --bin backcast`) で UI 起動。
3. Venue メニュー → `Connect Tachibana (Demo)` クリック → tkinter ダイアログが別ウィンドウで出る。
4. debug build なら ID/PW が prefill されている。OK 押下。
5. メニューバー右側の Venue バッジが `AUTHENTICATING` → `CONNECTED` に遷移。
6. session 永続化確認: `%LocalAppData%/the-trader-was-replaced/tachibana/tachibana_session.json` が作成され、URL のみ含み ID/PW を含まないこと。
7. **同時に**: Venue メニューを開くと `Connect kabuStation (Verify)` / `(Prod)` が灰色 disabled になっている (Step 7)。
8. `Disconnect` 押下 → `DISCONNECTED` に戻る + kabu メニューも再 enable。
9. `Connect Tachibana (Prod)` を試行: `TACHIBANA_ALLOW_PROD` 未設定なら prod ラジオが disabled、`=1` なら有効化。

**kabu 検証:**
- backend を `--live-venue KABU` で再起動 → kabuStation 本体 (検証/本番) を予め起動 → `Connect kabuStation (Verify)` → API パスワード prefill (debug) → OK → `CONNECTED`。
- 本体未起動状態でテスト → `KABU_STATION_NOT_RUNNING` 表示 + [再確認] ボタン動作。
- ディスク確認: `tachibana_session.json` 等、**kabu に類似する session ファイルが作られていない** こと (token はメモリ常駐のみ)。

**異常系検証:**
- ダイアログを 3 分以上放置 → `LOGIN_TIMEOUT` がトースト表示、backend は再 login 可能状態。
- ダイアログで Cancel → `USER_CANCELLED`、状態は `DISCONNECTED` に戻る。
- SSH 越し (headless) で debug build → tkinter 不可 → `NO_DISPLAY_AVAILABLE` → 自動的に env 経路で再試行され成功。
- 同 release build → `NO_DISPLAY_AVAILABLE` トーストで停止。

**Cross-venue 検証:**
- backend を `--live-venue TACHIBANA` で起動、UI から `Connect kabuStation (Verify)` を (Step 7 の disable を経て) 強行できないこと、強行された場合 (gating 抜けた race) は backend が `VENUE_MISMATCH` を返して error トーストが出ること。

---

## 影響ファイル一覧

**新規 (12):**
- `python/engine/live/_build_mode.py`
- `python/engine/exchanges/tachibana_login_form_state.py` (presenter / pure, tkinter なし)
- `python/engine/exchanges/tachibana_login_flow.py` (tkinter view + async ブリッジ)
- `python/engine/exchanges/kabusapi_login_form_state.py` (presenter / pure)
- `python/engine/exchanges/kabusapi_login_flow.py` (tkinter view + async ブリッジ)
- `tools/freeze_build_mode.py`
- `python/tests/live/test_build_mode.py`
- `python/tests/live/test_adapter.py` (`prompt_result` Literal smoke test、新規 if not existing)
- `python/tests/exchanges/test_tachibana_login_form_state.py` (display 不要)
- `python/tests/exchanges/test_tachibana_login_flow.py` (tkinter / display gating)
- `python/tests/exchanges/test_kabusapi_login_form_state.py` (display 不要)
- `python/tests/exchanges/test_kabusapi_login_flow.py` (tkinter / display gating)
- (任意) `python/tests/live/test_login_dialog_runner.py`

**編集 (11):**
- `python/engine/exchanges/tachibana.py` (`is_logged_in` property + `session_cache` 分岐実装)
- `python/engine/exchanges/kabusapi.py` (`is_logged_in` property + `prompt_result` 分岐実装)
- `python/engine/live/adapter.py` (`VenueCredentials.credentials_source` の Literal に `"prompt_result"` 追加 + `token: Optional[str] = None` 追加 + 必要なら `Optional` import)
- `python/engine/live/login_dialog_runner.py` (line 71-73 置換 + `--cred-path` (旧 `--cred-fd`) + venue 別 env enforce)
- `python/engine/live/live_adapter_factory.py` (signature 変更 + `_resolve_*` 追加)
- `python/engine/server_grpc.py` (`_handle_prompt_login` 追加 + VenueLogin を `_attempt` / `_fail` 内部関数で再構成 + `_start_live_components` signature 拡張 + `tempfile` / `os` import 追加)
- `python/tests/test_grpc_phase8.py` (テスト追加 + 既存 mock factory 呼出更新 + tempfile / teardown 検証)
- `python/tests/live/test_live_adapter_factory.py` (テスト追加)
- `python/tests/exchanges/test_tachibana_adapter.py`, `test_kabusapi_adapter.py` (is_logged_in / session_cache / prompt_result テスト追加)
- `src/trading.rs` (`is_venue_busy_for_menu` helper 追加)
- `src/ui/menu_bar.rs` (`gate_venue_menu_items_system` 追加 + handler の二重ガード + 既存 tests に gating ケース追加)

**conftest / CI gating (1):**
- 各 `test_*_login_flow.py` の冒頭で `pytest.importorskip("tkinter")` + `pytestmark = pytest.mark.skipif(not (os.environ.get("DISPLAY") or sys.platform in ("darwin", "win32")), reason="no display")` を置く。共通化したい場合は `python/tests/conftest.py` に `requires_display` fixture / marker を足す (任意)。

**proto / gRPC スキーマ変更: なし** (`credentials_source` / `environment_hint` は既存フィールド。`prompt_result` は backend 内部の adapter 表層語彙で、UI は引き続き `prompt` を送る)。

**`Cargo.toml` / `pyproject.toml` 変更:**
- いずれも依存・hook 追加なし。release pipeline 側で `tools/freeze_build_mode.py` を呼ぶ運用に切り替える。

---

## 制約・既知事項

- **1 backend = 1 venue** は仕様 (§7 ADR / D26)。本タスクはこれを覆さない。UI gating (Step 7) で操作可能なメニューを明示する。
- **release ビルド**ではすべての `DEV_*` env が無効化される (§3.2 / §7 ADR line 1564)。リリース利用者は毎ログイン時に tkinter フォームで ID/PW を手入力する。
- **第二暗証番号 (Tachibana) / 取引パスワード (kabu)** は本タスク非対象 (Phase 9 で発注 UI から取得)。
- **headless 環境** (CI / SSH 越し) では tkinter `Tk()` 構築失敗 → `NO_DISPLAY_AVAILABLE` 返却 → debug build なら `env` 経路 fallback (Step 5 `_attempt("env")` 再呼出)、release ではエラー。テスト戦略: presenter 層 (`*_form_state.py`) は display 不要・CI 必須、view 層 (`*_login_flow.py`) は `pytest.importorskip` + display gating でローカル / macOS / Windows のみ実行。
- **async / tkinter ブリッジ**: tkinter callback は同期。async auth は `threading.Thread(target=lambda: asyncio.run(...))` で別スレッド実行し、結果は `root.after(0, _on_auth_done)` で UI スレッドに戻す。callback 中は OK/Cancel ボタンを `state="disabled"` にして二重 submit を防ぐ。`asyncio.run` を tkinter callback 内で同期呼出すると mainloop が一時停止し UI が固まるため、必ずスレッド分離する。
- **IPC は cross-platform tempfile**: `pass_fds` (POSIX 限定) は使わない。kabu 経路で `tempfile.mkstemp` + `--cred-path` + `O_TRUNC` 書込 + parent 側 `os.unlink` 後始末。POSIX のみ `chmod 0o600`、Windows は既定 ACL に依存。
- **kabu の token はメモリ常駐のみ**。再起動・Disconnect で消失し、再 login が必要 (skill S4)。Tachibana は当日中に限り session ファイルで再利用可。
- **VenueLogin gRPC 呼び出しは最大 3 分ブロックする** (`LIVE_LOGIN_TIMEOUT_S` 既定)。grpc.server の threadpool worker thread を占有するが、他 RPC は別 worker で進む。Rust UI 側は既存の transport task で待機。

---

## サブエージェント運用 (推奨)

Step 数が 8 (Python adapter 修正 + Python flow 新規 3 + 既存 wiring 修正 + Rust UI 1)、新規テスト 5-6 ファイルと中規模。`parallel-agent-dev` skill が向く:

- **Lane A (adapter 基盤)**: Step 0 (両 adapter の `is_logged_in` + session_cache / prompt_result)。tdd-workflow + tachibana + kabusapi skill 準拠。**他 Lane の前提**なので最初に完了させる。
- **Lane B (Python dialog flows)**: Step 1, 2, 3, 4 (`_build_mode` + 2 login flow + runner)。tdd-workflow + tachibana + kabusapi skill 準拠。Lane A 完了後に着手。
- **Lane C (Python wiring)**: Step **6 → 5** の順 (factory + `_start_live_components` シグネチャ拡張を先に入れないと Step 5 の呼び出しで TypeError)。server_grpc subprocess IPC + factory。tdd-workflow skill 準拠。Lane A/B 完了後に着手 (Step 5 のテストが Lane A/B の API に依存)。
- **Lane D (Rust UI)**: Step 7, 8 (menu_bar gating + trading helper)。bevy-engine + rust-testing skill 準拠。Lane A/B/C と独立に並行実行可能。

または、より逐次的に進めるなら `pair-relay` で Step 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7 の順に Driver/Navigator 分担。
