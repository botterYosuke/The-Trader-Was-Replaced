# Backend Startup Synchronization (Phase 8 §3.8 補強)

Bevy UI と Python `server_grpc` の **起動シーケンスの待ち合わせ** を仕様化する。Phase 8 §3.8 (`docs/plan/Phase 8 - Live Venue and Market Data.md:1184-1263`) と Step 4.6 (`:1359-1369`) はプロセス所有権・crash 検知・singleton までは設計したが、「Bevy が `python -m engine` を spawn してから最初の RPC を打って良いまでの待ち合わせ」「既存 backend への attach」「stdout 配線」「Python interpreter 解決」「`Shutdown` RPC の proto 追加」が空白のまま。本計画はこの 5 点だけを埋める単独タスクとして切り出す。Phase 8 本計画の §3.8.11 として後で逆輸入できる粒度で書く。

## Context

現状 ([src/trading.rs:108-148](src/trading.rs#L108-L148)) は backend を **外部で起動済み** という前提で `BACKEND_URL=http://127.0.0.1:19876` に決め打ち接続している。`backend_enabled` も env で手動制御。Phase 8 Step 4.6 で `src/backend_supervisor.rs` を新設して Bevy 自身が `python -m engine` を spawn する経路を作るが、

- spawn 直後の数秒間は server_grpc が **まだ port bind を終えていない** ため、Bevy が即 `GetState` を打つと `tonic` 側で `Status::unavailable` が返り、Phase 8 §3.8.5 の crash 検知 (`200ms × 3`) がそのまま **起動時の false positive** を起こして `BACKEND_CRASHED` トーストが点滅する。
- `python -m engine` の **stdout/stderr** を Bevy 側で drain しないと OS pipe バッファが満杯になり Python が `print()` で永久ブロックする（古典）。Phase 8 計画には言及なし。
- 開発時に `python -m engine` を別ターミナルで先に起動している場合、Bevy が再 spawn を試みると `:19876` を 2 重 bind して `BACKEND_ALREADY_RUNNING` で死ぬ。`§3.8.9` は「独立起動は開発時専用」とだけ書いて、**Bevy 側の pre-check & attach 経路** を定義していない。
- `BACKEND_URL` 解析だけでは Bevy が「spawn すべきか attach すべきか」を判定できない。Bevy は `:19876` を **TCP connect probe** で先に叩く必要がある。
- `§3.8.4` は「Phase 8 で新設する `Shutdown` RPC」と書くが、同じ §3.8.4 の「修正:」コメント ([:1216](docs/plan/Phase%208%20-%20Live%20Venue%20and%20Market%20Data.md#L1216)) で「追加するか、SIGINT/SIGBREAK 経路に一本化するか」が**両論併記のまま未決**で残されている。Step 2 の proto 追加リスト ([:1335](docs/plan/Phase%208%20-%20Live%20Venue%20and%20Market%20Data.md#L1335)) にも含まれていない。本計画はこの未決を **Shutdown RPC 採用側に確定** させる位置付け (理由は C-6 / ADR 参照)。
- `Health.Check` RPC は **Python 側に既に実装済み** ([python/proto/engine.proto:5-13](python/proto/engine.proto#L5-L13), `HealthServicer` 継承は [python/engine/server_grpc.py:143](python/engine/server_grpc.py#L143)、`Check` メソッド実装は [python/engine/server_grpc.py:273-276](python/engine/server_grpc.py#L273-L276) で 4 行・無条件 SERVING 返却) なのに、Rust 側からは一度も呼ばれていない (`grep -rn Health src/` → 0 件)。Readiness probe の手段はもう揃っており、配線だけが残っている。
- Python interpreter 解決ルールが未定義。`PATH` 依存だと `.venv` を当てずに global Python を拾い `ModuleNotFoundError` で死ぬ。

---

## 設計契約 (実装前に固める)

### C-1. Readiness の判定手段

**最終 Ready 判定は `Health.Check(service="DataEngine") = SERVING` を受けた直後に token 付き `GetState` を 1 回打って Status::OK が返ることをもって成立** (= 2 段判定。spawn 経路 / attach 経路ともに同じ条件)。stdout sentinel は補助 (= probe を始めて良いという最速 signal) であり、Ready 判定の独立経路にはしない。`Health.Check` だけでは `add_DataEngineServicer_to_server()` の呼び忘れや proto/stub 不整合を検知できないため、`GetState` handshake を Ready 条件に必ず含める (このセクション末尾と C-3 で詳述)。理由:

- TCP connect は `add_insecure_port` 直後 (`server.start()` 前) でも成功し得るため「bind は終わったが servicer 未登録」の偽陽性を引く。
- gRPC channel の `tonic::transport::Channel::connect_lazy` は実 RPC を打つまで失敗を返さないため、bind 完了確認に使えない。
- stdout sentinel 単独で Ready としてしまうと、attach 経路 (stdout を持たない) とロジックが分岐し test カバレッジが二重化する。さらに sentinel は「server.start() の直後」を保証するだけで、Python 側で意図せず `add_DataEngineServicer_to_server()` の前に sentinel を吐く順序ミスがあった場合に検知できない。
- `Health.Check(service="DataEngine")` を **200ms 間隔で最大 75 回 (15 秒)** リトライし、`SERVING` を 1 回受けたら Ready とみなす。
- 15 秒以内に SERVING に到達しない場合は `BACKEND_STARTUP_TIMEOUT` で Failed 状態へ。crash 検知 (§3.8.5) とは **エラーコード・トースト文言を分ける**（起動失敗と稼働中クラッシュは原因が違うため）。
- **budget を 15 秒に取る理由**: Windows の `python.exe` cold start (1-2s) + nautilus_trader / engine package import (2-4s) + `--jquants-catalog-path` ParquetDataCatalog の discovery / index load (~1s) + gRPC server bind + Health servicer 登録の総和は、実機の冷キャッシュ環境では 5-8 秒に達する。budget 5 秒では false `StartupFailed` を量産しうる。一方 attach 経路 (C-3) は「立っている前提」なので 2 秒のままで良い。なお `--jquants-catalog-path` は `python/engine/__main__.py:39` の通り Parquet catalog ディレクトリを指す引数で、CSV ingester (`--jquants-dir`) とは別物。

**spawn 経路でも GetState handshake を Ready 条件に含める理由**: `Health.Check(service="DataEngine") = SERVING` は Health servicer が文字列 `"DataEngine"` を受理したことしか保証しないため、`add_DataEngineServicer_to_server()` の呼び忘れや proto 生成ミス (stub 不整合) を検知できない。attach 経路 (C-3) と同じく **`GetState` 1 回 OK を Ready 条件に含める** ことで spawn 経路でも登録漏れを起動時に弾く。これにより Ready 後の polling で `UNIMPLEMENTED` が突然出てくる体験を構造的に防ぐ。spawn 経路では `GetState` の token は Bevy が `--token` 引数で渡した値なので、`UNAUTHENTICATED` が出れば proto/stub 側の構造的バグである。

**`Health.Check` の service フィルタ強化** (Step 3 で実装、現状実装は service 無視で常に SERVING):
- `request.service == "DataEngine"` または `== ""` (= default service) のとき `SERVING` (shutdown 中は `NOT_SERVING`)
- それ以外は `SERVICE_UNKNOWN`

> **proto enum 拡張が必要**: 現行 `python/proto/engine.proto:14-18` の `HealthCheckResponse.ServingStatus` は `{UNKNOWN=0, SERVING=1, NOT_SERVING=2}` のみで `SERVICE_UNKNOWN` は **未定義**。Step 2 で `SERVICE_UNKNOWN = 3` を追加する (proto / stubs 再生成 / Rust 側 enum 自動更新)。これは proto 変更だが既存値は wire 互換 (append-only)。
>
> **注意 — 独自 Health service の enum 拡張**: 本 repo の `Health.Check` は `grpc.health.v1.Health` (grpc-health-checking 標準パッケージ) ではなく、`engine.proto` に **独自定義** された `service Health` ([python/engine/server_grpc.py:143](python/engine/server_grpc.py#L143) で `HealthServicer` を継承しているが、その `HealthServicer` 自体が repo 内 stub から生成された独自 class)。enum 値 `SERVICE_UNKNOWN=3` は標準 health proto の `SERVICE_UNKNOWN` と数値・名前が偶然一致するだけで、wire レベルの互換ではない。外部の grpc-health-checking 標準クライアント (grpcurl の `grpc.health.v1.Health/Check` 経路など) は本 backend に対しては動かない。

**identity の最終判定は `Health.Check` ではなく token 付き `GetState` handshake**: `Health` と `DataEngine` は proto 上別 service として登録される (`python/proto/engine.proto` の `service Health { ... }` と `service DataEngine { ... }`) ため、`Health.Check(service="DataEngine") = SERVING` は **「Health servicer が "DataEngine" という名前を受理した」** ことしか保証しない。実際に `DataEngine` servicer が登録され RPC を返せるかは別問題。よって誤 attach の最終チェックは attach 経路 (C-3) で打つ **token 付き `GetState`** が成功することで確認する。Health の `service` フィルタは「全くの別 gRPC service (e.g. envoy admin) が同 port を握っている」case を **早期に弾く** ための sanity チェック扱いで、それ自身が identity proof ではない。

### C-2. Bevy 側起動状態機械 `BackendLifecycle`

`src/backend_supervisor.rs` に Resource として持つ:

```text
Disabled (BACKEND_ENABLED=false で固定、終端状態)

NotStarted
   ↓ BACKEND_URL を url::Url でパース失敗 (scheme / port 欠落 等)
StartupFailed(BACKEND_URL_INVALID)   ← probe 開始前に確定するため独立分岐

NotStarted
   ↓ TCP connect probe (BACKEND_URL の host:port、100ms timeout)
ProbingExisting ── 接続成功 → Health.Check(service="DataEngine") SERVING → GetState 1 回成功 ──→ Ready (attach 経路、Job Object には入れない)
   ↓ TCP connect 失敗
   ↓ BACKEND_AUTOSPAWN=0 なら → StartupFailed(BACKEND_NOT_REACHABLE)   ← spawn 経路に降りない (env が spawn 権限を切っているため)
   ↓ BACKEND_AUTOSPAWN=1
Spawning
   ↓ stdout sentinel ^GRPC_LISTENING port=(\d+)$ 検知 (probe 開始の最速 signal、省略可)
   ↓ Health.Check(service="DataEngine") を 200ms 間隔で最大 75 回 (15 秒 budget)
   ↓ SERVING を 1 回受信
   ↓ 続けて token 付き GetState を 1 回 (handshake)、Status::OK を確認
Ready
   ↓ 15 秒以内に SERVING に到達しない        → StartupFailed(BACKEND_STARTUP_TIMEOUT)
   ↓ SERVING は来たが GetState が UNIMPLEMENTED → StartupFailed(BACKEND_SERVICER_MISSING)
                                                  (= add_DataEngineServicer_to_server() 漏れ等の登録ミス)
   ↓ SERVING は来たが GetState が UNAUTHENTICATED → StartupFailed(BACKEND_TOKEN_MISMATCH)
                                                  (spawn 経路では Bevy が token を渡しているので構造的には
                                                  起きないはずだが、proto/stub の不整合検知の保険)
StartupFailed (Restart ボタンで Spawning へ戻れる、§3.8.6 と共通カウンタ)

Ready
   ↓ Health.Check が 200ms 間隔で 3 連続失敗 (合計 ~600ms 以内に判定、§3.8.5 を本計画 Step 5 で書き換え)
Crashed (Restart ボタンで Spawning へ戻れる、クラッシュループカウンタは §3.8.6 と共通)

Ready
   ↓ Shutdown RPC ハンドラまたは SIGINT で start_shutdown() 起動
   ↓ Health.Check が NOT_SERVING を返し始める
ShuttingDown
   ↓ subprocess exit (exit_code=0) または NOT_SERVING を 30 連続受信 (~6 秒、200ms tick × 30)
Stopped (Restart ボタンで Spawning へ戻れる、クラッシュループカウンタには加算しない)
```

`ShuttingDown` / `Stopped` を `Crashed` と分けるのは、graceful shutdown 中の `NOT_SERVING` を Crashed loop に加算しないため (ユーザが意図的に止めただけで再起動制限を発動させないため、§3.8.6 と整合)。

**enum と watch channel**: `BackendLifecycle` は **`StartupFailed(&'static str)` を inline payload として持つ** enum とする (`Disabled / NotStarted / ProbingExisting / Spawning / Ready / ShuttingDown / Stopped / Crashed / StartupFailed(&'static str)`)。エラーコード文字列 (`BACKEND_URL_INVALID` / `BACKEND_NOT_REACHABLE` / `BACKEND_STARTUP_TIMEOUT` / `BACKEND_SERVICER_MISSING` / `BACKEND_TOKEN_MISMATCH` / `BACKEND_HANDSHAKE_FAILED` / `BACKEND_IDENTITY_MISMATCH` / `BACKEND_VENV_NOT_FOUND` / `BACKEND_CWD_NOT_FOUND`) は `&'static str` リテラルなので payload があっても `Clone + PartialEq + Eq + Debug` は derive 可能。`Copy` は捨てる代わりに **lifecycle と reason の同時更新が atomic に伝播する** ことを優先する (2 経路を別 watch にすると subscriber が中間状態を読む race が起きる)。`watch::Receiver::wait_for(|s| matches!(s, BackendLifecycle::Ready))` は `PartialEq` で十分成立する。

**`Ready` に入るまで `GetState` 60Hz polling を開始しない** (gating の実装詳細は Step 4 参照、`setup_backend_connection` の Tokio task を `tokio::sync::watch::Receiver<BackendLifecycle>` で Ready 待ちにする)。これにより起動時の false-positive `BACKEND_CRASHED` トーストを構造的に塞ぐ。

**Restart 遷移表** (`[Restart Backend]` ボタン押下時の挙動、`(現 state, own_process, BACKEND_AUTOSPAWN)` → 次 state の網羅マトリクス):

| 現 state | own_process | AUTOSPAWN | ボタン状態 | 押下時の遷移 |
|---|---|---|---|---|
| `Disabled` | — | — | disabled (固定終端) | 何もしない (env を直して Bevy 再起動) |
| `NotStarted` / `ProbingExisting` / `Spawning` | — | — | disabled (進行中) | 何もしない |
| `Ready` / `ShuttingDown` | — | — | disabled (健全 or graceful 中) | 何もしない |
| `Stopped` | `true` | — | enabled | `NotStarted → ProbingExisting → ...` (probe からやり直し) |
| `Stopped` | `false` | — | enabled | `NotStarted → ProbingExisting → ...` (同上、attach 先が再起動済みの可能性) |
| `Crashed` | `true` | — | enabled (CRASH_LOOP 未発動) | `NotStarted → ProbingExisting → Spawning` (`own_process=true` は `attach_retry_count` の 2 回 gating をスキップし、最初の probe 失敗で即 spawn に降下。Bevy 自前で立てた backend が死んだケースなので外部 attach の可能性を考慮しない) |
| `Crashed` | `false` | `1` | enabled (CRASH_LOOP 未発動) | `NotStarted → ProbingExisting → ...`。attach probe が 2 度連続失敗したら Spawning に降下 + `own_process=true` (C-3) |
| `Crashed` | `false` | `0` | enabled | `NotStarted → ProbingExisting → ...`。attach probe 失敗時は `StartupFailed(BACKEND_NOT_REACHABLE)` に倒す (Spawning に降りない) |
| `StartupFailed(BACKEND_URL_INVALID)` | — | — | disabled (env エラー) | 何もしない (env を直して Bevy 再起動) |
| `StartupFailed(BACKEND_NOT_REACHABLE)` | `false` | `0` | disabled (Step 6 ADR 通り spawn 経路無効化) | 何もしない (env / 外部 backend を直す) |
| `StartupFailed(BACKEND_NOT_REACHABLE)` | `false` | `1` | enabled | `NotStarted → ProbingExisting → ...` (probe からやり直し、`attach_retry_count` は saturate 2 のまま継続参照) |
| `StartupFailed(BACKEND_VENV_NOT_FOUND / BACKEND_CWD_NOT_FOUND)` | — | — | disabled (env エラー) | 何もしない |
| `StartupFailed(BACKEND_STARTUP_TIMEOUT / BACKEND_SERVICER_MISSING / BACKEND_HANDSHAKE_FAILED)` | `true` | — | enabled (CRASH_LOOP 未発動) | `NotStarted → ProbingExisting → Spawning` (CRASH_LOOP カウンタに加算、§3.8.6) |
| `StartupFailed(BACKEND_TOKEN_MISMATCH / BACKEND_IDENTITY_MISMATCH)` | `false` | — | disabled (env / 同 port 別プロセスを直す必要) | 何もしない |
| (どの state でも) CRASH_LOOP_DETECTED | — | — | disabled (§3.8.6) | 何もしない (Bevy 再起動を強制) |

「probe からやり直し」= `NotStarted` に戻ってから C-2 図のフロー全体を再実行。Restart 経由の `NotStarted` でも URL パース → TCP probe → SERVING + GetState handshake をすべて再評価する (Restart の意図がそもそも「外部状態が変わったかもしれない」ため)。

### C-3. 既存 backend への attach 経路

Bevy 起動時に `BACKEND_URL` (default `http://127.0.0.1:19876`) を `url::Url` でパースし、`host:port` を取り出して 100ms timeout で `TcpStream::connect` を試す:

- 成功 → `ProbingExisting` → `Health.Check(service="DataEngine")` を **200ms 間隔で最大 10 回 (合計 2 秒)** リトライし (spawn 経路 C-1 と同じ tick 周期で budget だけ短く)、SERVING を 1 回受信 → 続けて token 付き `GetState` を 1 回打って成功すれば `Ready` (Job Object に入れない、Bevy 終了時に backend を kill しない、後述 `own_process=false` を Resource に記録)。retry budget を設けるのは、外部 backend が TCP bind 直後・gRPC servicer 登録前という瞬間に Bevy 起動が重なった場合の偽 StartupFailed を避けるため (spawn 経路の 15 秒に対し、外部 backend は「立っている前提」なので予算を短く)。2 秒以内に SERVING に到達しなければ `StartupFailed(BACKEND_STARTUP_TIMEOUT)`。
  - `Health.Check` SERVING を受けたが `GetState` が `UNAUTHENTICATED` を返した場合: `StartupFailed(BACKEND_TOKEN_MISMATCH)` (外部 backend が異なる `BACKEND_TOKEN` で起動している。ユーザは env を揃えて再起動する)。これは `SERVICE_UNKNOWN` 由来の `BACKEND_IDENTITY_MISMATCH` (= 別 gRPC service が同 port を掴んでいる) とは別エラーコードとして扱う。
  - `GetState` がその他の `Status` で失敗した場合: `StartupFailed(BACKEND_HANDSHAKE_FAILED)` で停止。
- 失敗 → `Spawning` (新規 spawn、`own_process=true` を Resource に記録、Job Object に attach — ただし Job Object 本体実装は Phase 8 Step 4.6 の所管、本計画完了時点では未配線。Non-Goals 参照)

**`BackendOwnership { own_process: bool }` Resource**: Bevy が自分で spawn したか、外部にある backend に attach したかを記録する。`AppExit` ハンドラ (C-8) はこれを見て、`own_process=true` の場合だけ `Shutdown` RPC / `Child::kill()` を実行する。`own_process=false` (attach) のとき Bevy 終了で外部 backend に干渉しないのは ADR「attach 時は Job Object に入れない」の自然な延長。

**attach 経路で外部 backend が死んだ後の挙動** (`own_process=false` のまま `Crashed` / `Stopped` に入った場合):

- Footer の `[Restart Backend]` 押下時は **必ず attach probe からやり直す** (`Crashed → NotStarted → ProbingExisting → ...`)。`own_process=false` の状態で Bevy が勝手に `Spawning` 経路に降りて新しい python を立てると、ユーザーが別ターミナルで再起動した backend と `:19876` を二重 bind するか、ユーザーが意図しない backend を Bevy が所有することになる。
- attach probe が **2 度連続で失敗** したら、そこで初めて `BACKEND_AUTOSPAWN` を評価する: `=1` なら `Spawning` に降りて `own_process=true` に切り替え、`=0` なら `StartupFailed(BACKEND_NOT_REACHABLE)`。**ただし `own_process=true` (Bevy 自前で立てた backend が死んだケース) の Restart はこの 2 回 gating の対象外** — 外部 attach の可能性を考慮する意味がないため、最初の probe 失敗で即 `Spawning` に降下する (C-2 Restart 表の `Crashed (own_process=true)` 行と整合)。
- 「2 度連続」のカウンタ (`attach_retry_count`) は §3.8.6 のクラッシュループとは別軸 (短時間の Restart 連打を抑止する目的ではないため)。supervisor task が保持する `u32` フィールドで、**2 で saturate** する (それ以上は加算しない、StartupFailed 経由の Restart でも増えない)。リセット契機は次の 2 つに限定する: (a) attach probe が `ProbingExisting → Ready` に成功 (= SERVING + GetState OK) したとき、(b) `own_process=false` から `own_process=true` への切り替え (= Spawning 経路降下) が確定したとき。`Crashed` だけでなく `StartupFailed(BACKEND_NOT_REACHABLE)` 経由の Restart も同じ `attach_retry_count` を参照する (= 押下時に毎回 attach probe を試み、2 回連続失敗を超えた時点で `BACKEND_AUTOSPAWN=1` なら Spawning 降下)。これにより外部 backend が一度成功すれば、その後の oscillation でも spawn 経路に永久ロックされない。Bevy プロセス再起動 (supervisor task 再生成) でも 0 から始まるので、永続化はしない。

これにより「外部 backend が落ちて Restart 押下 → ユーザーがまだ別ターミナルで再起動中 → 2 回目の Restart で自前 spawn に切り替え」という遷移が決定的になる。

**Crashed loop カウンタ (§3.8.6) と own_process の関係**: カウンタは `own_process` の値によらず加算する。attach 経路で外部 backend が短時間に何度も死ぬのも、運用上は同じ「ユーザー観察を強制すべき」状況のため。

URL パースの不変条件:
- スキームは `http://` のみ受理。`https://`, `grpc://` 等は `StartupFailed(BACKEND_URL_INVALID)` で reject (Phase 8 では TLS なし)。
- IPv6 リテラルは bracket 形式 `http://[::1]:19876` を許容。
- port 省略 (`http://127.0.0.1/`) は `BACKEND_URL_INVALID`。

**Cargo.toml 依存追加** (Step 4 で配線、いずれも既存になければ追加):
- `url = "2"` (URL パースに使用)。
- `wait-timeout = "0.2"` (`Child::wait_timeout` のため、C-6 末尾の記述と同じ)。
- `regex = "1"` (stdout sentinel parser のため。既存に含まれていれば再利用)。
- `tokio` は既存依存。sentinel channel / cmd channel / watch channel すべて `tokio::sync::mpsc` / `tokio::sync::watch` / `tokio::sync::oneshot` で完結するため、`crossbeam-channel` / `crossbeam` の追加は不要。

Step 4 着手前に `Cargo.toml` を grep して未追加分だけ加える (重複追加を避ける)。

`engine.pid` の生存確認 (§3.8.2) は **Python 側の責務** で、Bevy はここに触らない。

env `BACKEND_AUTOSPAWN` の意味 (default `1`):
- `BACKEND_AUTOSPAWN=1`: attach probe → 成功時は `ProbingExisting → Ready`、失敗時は `Spawning` (通常運用)。
- `BACKEND_AUTOSPAWN=0`: attach probe → 成功時は `ProbingExisting → Ready` (CI / e2e の正常パス、外部 backend が必ず立っている前提)、失敗時は `StartupFailed(BACKEND_NOT_REACHABLE)` で止める。
- attach probe 自体は `BACKEND_AUTOSPAWN` の値によらず常に走る (env は spawn 経路の許可フラグ)。

**既存 `BACKEND_ENABLED` (`src/trading.rs:122`、default `false`) との関係**:
- `BACKEND_ENABLED=false`: `BackendLifecycle` を **`Disabled` に固定** し、probe / spawn を一切走らせない。Footer の backend status は `Disabled` (灰) を表示。`setup_backend_connection` の現行早期 return ([src/main.rs:357](src/main.rs#L357) 付近) は維持。
- `BACKEND_ENABLED=true`: 本計画の lifecycle が起動 (`NotStarted → ProbingExisting → ...`)。`BACKEND_AUTOSPAWN` は `BACKEND_ENABLED=true` のときだけ意味を持つ。
- `Disabled` は終端状態 (env は起動時 snapshot)。Footer から runtime トグルは不要。

### C-3b. `python -m engine` の起動コマンドライン

`python/engine/__main__.py:8-54` の現行 CLI 仕様:
- `--token <str>` **(required)**
- `--port <int>` (default `19876`、`BACKEND_PORT` env は **読まない**)
- その他 `--max-history-len` / `--advance-interval-sec` / `--jquants-catalog-path` / `--live-venue` 等は当面 default で OK (Phase 8 で個別に env 配線)

Bevy 側 `spawn_python_backend_system` は次の引数で起動する:

```text
<PYTHON_BIN> -m engine \
    --token <BACKEND_TOKEN env, default "dev-token"> \
    --port <BACKEND_URL の port を url::Url::port_or_known_default() で抽出> \
    [--jquants-catalog-path <TradingSettings.catalog_path があれば>] \
    [--live-venue <Phase 8 後段で配線、本計画ではスキップ>]
```

env として `PYTHONPATH=<CWD>/python` を追加 (C-4)。`BACKEND_TOKEN` / `BACKEND_URL` の解決は `TradingSettings::from_env()` ([src/trading.rs:120-128](src/trading.rs#L120-L128)) と同じロジックを再利用する (重複定義しない)。

### C-4. Python interpreter 解決順

`src/backend_supervisor.rs` で次の順に試し、最初に存在するものを使う:

1. env `PYTHON_BIN` が定義されていればそれ
2. `cwd/.venv/Scripts/python.exe` (Windows) / `cwd/.venv/bin/python` (POSIX)
3. `cwd/venv/...` 同じく (legacy venv ディレクトリ名)

**`PATH` fallback (`which python`) は採用しない**。`.venv` を見つけられなかった場合は `StartupFailed(BACKEND_VENV_NOT_FOUND)` で停止し、ユーザに `PYTHON_BIN=` の明示を促す。理由: global Python を拾うと `python -m engine` が `ModuleNotFoundError: No module named 'engine'` で死に、5 秒 timeout (Health.Check 失敗) を消費してから `BACKEND_STARTUP_TIMEOUT` になる。原因が「venv 未解決」だったのか「server.start() の遅延」なのか区別できなくなるため、PATH fallback を許す代わりに明示エラーで止める方が診断が早い。

`PYTHON_BIN` が指定された場合 (CI / 非標準セットアップ用) のみ、起動前に **preflight check** `<PYTHON_BIN> -c "import engine"` を **5 秒 timeout** で実行し、import 失敗 (timeout 含む) なら `StartupFailed(BACKEND_VENV_NOT_FOUND)`。これで `PYTHON_BIN` が global Python を指していて engine package 未インストールの case も早期検知できる。5 秒は Windows python.exe cold start (1-2s) + engine package import (2-4s) を含めても余裕がある値。preflight が timeout した場合は誤検知の余地があるため、ログに `[backend] PYTHON_BIN preflight timed out — assuming venv mismatch` を出してから StartupFailed する。

**preflight 実行時の env**: 本体子プロセス起動時と同じ env (特に `PYTHONPATH=<CWD>/python`、後述) を **必ず継承させる**。preflight だけ素の env で走らせると、`engine` package が venv の site-packages ではなく repo の `python/engine/` 直下にしかない構成で false negative になる (本 repo は editable install 前提ではなく `PYTHONPATH` 経由で engine を import する想定)。`std::process::Command::env("PYTHONPATH", ...)` を preflight にも本起動と同じ値で適用すること。

CWD (= `.venv` 探索の基点) の解決は次の順で 1 回だけ行い、起動後は固定:

1. env `BACKEND_CWD` があればそれ。
2. release ビルド: `std::env::current_exe()` の親ディレクトリから上に遡って `Cargo.toml` を含むディレクトリを探す。見つからなければ `StartupFailed(BACKEND_CWD_NOT_FOUND)` で停止し、env `BACKEND_CWD=` の明示を促す (silent fallback で `current_exe().parent()` を使うと、見当違いの場所を CWD にして `.venv` 未発見 → `BACKEND_VENV_NOT_FOUND` という二段エラーになり原因切り分けが遅れるため fail fast する)。
3. debug / `cargo run` 経由: `env!("CARGO_MANIFEST_DIR")` (コンパイル時固定値、開発時用)。release では使わない (ビルドマシンの絶対パスが焼き込まれるため)。

これは「リポジトリと .venv が常に同一階層にある」前提のヒューリスティック。

子プロセス起動時の env として **`PYTHONPATH=<CWD>/python` を明示的に追加**する (engine package が `python/engine/` 直下にあるため)。これで venv 内 python に依存しない preflight (`python -c "import engine"` を任意の `PYTHON_BIN` で実行) が成立する。

### C-5. stdout / stderr 配線

`std::process::Command::stdout(Stdio::piped()).stderr(Stdio::piped())` で取得し、**spawn 直後に 2 本の thread で `BufReader::lines()` を drain** する。pipe バッファ満杯による Python ブロックを防ぐのが主目的。プロジェクトは `bevy::log` (内部で `tracing` 利用) を採用しているので、本計画も `bevy::log::{info,warn}!` で転送する (`tracing` crate を直接 import しない / `target:` 指定は使わず文字列 prefix `[backend]` で代用)。

- stderr は `bevy::log::warn!("[backend] {}", line)` でそのまま転送 (logging masking は Python 側責務、§3.2.1 / Tachibana skill R10)。
- stdout は次の 2 通りに振り分け:
  - 正規表現 `^GRPC_LISTENING port=(\d+)$` にマッチする行を C-1 の readiness 補助として検出（最速の確定経路）。抽出した port を `BACKEND_URL` の port と比較し、不一致なら `bevy::log::error!` で警告 (sentinel は無視、Health.Check 経路で readiness 判定継続)。
  - それ以外は `bevy::log::info!("[backend] {}", line)` で転送。
- Python 側で `print(f"GRPC_LISTENING port={port}", flush=True)` を [server_grpc.py:1169](python/engine/server_grpc.py#L1169) の `server.start()` 直後に **1 行だけ** 追加する。`port` は `server.add_insecure_port(...)` に渡した値そのもの (env override 後の解決値)。flush 必須 (Windows の python.exe は line-buffered ではないことがある)。
- format `GRPC_LISTENING port=<port>` は **Rust/Python 間の interface 契約**。Step 1 で Python 側 integration test、Step 4 で Rust 側 unit test (parser の golden) でカバー。
- 既存 backend に attach した場合 (C-3) は stdout pipe を持たないので、readiness は `Health.Check` 経路のみで成立する。

### C-6. `Shutdown` RPC の追加

Phase 8 §3.8.4 が両論併記のまま残している「Shutdown RPC を新設するか / SIGINT 一本化か」の二択を、本計画で **Shutdown RPC 採用側に確定** させる (詳細な理由は ADR セクション参照)。SIGINT/SIGBREAK ハンドラは Ctrl+C や `taskkill` (no `/F`) などの OS 経由の停止受信路として残し、Bevy → Python の正規 shutdown 経路は Shutdown RPC を一次手段にする。両経路は同じ `process_lifecycle.start_shutdown()` を呼ぶので shutdown 順序のロジック分岐は起きない (関数名・公開 API 詳細は Step 3 を参照)。

`service DataEngine` に追加:

```proto
rpc Shutdown(ShutdownRequest) returns (ShutdownResponse);

message ShutdownRequest {
  string token = 1;
  uint32 grace_seconds = 2;  // 必ず明示送信。0 = 即時 (in-flight RPC 打ち切り)。
}
message ShutdownResponse {
  // accepted=true のとき error_code は空文字列必須 (wire 契約)。
  // accepted=false のとき error_code は次のいずれか:
  //   "INVALID_TOKEN"        — token 不一致
  //   "ALREADY_SHUTTING_DOWN" — 既に shutdown thread が起動済み
  bool accepted = 1;
  string error_code = 2;
}
```

**`grace_seconds` のセマンティクス** (proto3 default 問題の解消):
- proto3 は `uint32` の unset と `0` を区別できない。よって本計画では **「`grace_seconds` は client が必ず明示的に値をセットして送る」** ことを wire 契約とする (Bevy 側 client は AppExit 経路で `grace=0` を、運用上の手動 shutdown では `grace=3` を、明示的に build して送る)。
- handler 側で `request.grace_seconds` を読んだ値をそのまま `start_shutdown(grace_seconds=...)` に渡す。0 を「即時」として尊重する (= `server.stop(0)` で in-flight 打ち切り)。
- 「default 3」は **Python 側 `start_shutdown(grace_seconds: int = 3)` のキーワード default のみ**に存在し、これは SIGINT/SIGBREAK ハンドラ経由で grace を指定しなかった場合の fallback。Shutdown RPC ハンドラからは `request.grace_seconds` を常に明示渡しするため、RPC 経路ではこの default は発火しない。
- Bevy AppExit 経路で `grace=0` を送るのは C-8 の通り (Bevy が短いタイムアウトで自前管理し、Python 側に長い grace を投げて二重に待たないため)。SIGINT 経路は grace 指定なしで呼び出されるため Python の default `3` が効く。

**現行 Python 実装は同期 gRPC server** ([server_grpc.py:1154](python/engine/server_grpc.py#L1154) `grpc.server(futures.ThreadPoolExecutor(...))`, RPC ハンドラはすべて `def` で同期、live loop は別 thread で asyncio を回している `_ensure_live_loop()` ハイブリッド構造)。よって shutdown は asyncio (`grpc.aio`) 前提では書けない。**別 thread を spawn して shutdown 工程を同期 API で実行する**設計にする:

shutdown 実行順序 (process_lifecycle の `_shutdown_thread_main()` 関数、Step 3 の実装参照):

1. `_shutting_down = True` フラグを立て、以降の `Health.Check` を `NOT_SERVING` に切り替える (Bevy 側で `Crashed` 誤判定を避ける)。
2. **`time.sleep(0.25)`** — Shutdown RPC ハンドラの `ShutdownResponse(accepted=True)` が wire に乗る猶予 (`server.stop(0)` で in-flight RPC を打ち切る前にレスポンスを返し切るため)。同期 gRPC server は HTTP/2 フレーム送信を別 thread で処理しており、ローカル loopback でも Windows の slow CI 環境では HTTP/2 DATA フレーム flush に 100ms 超かかる事例が確認されている。250ms を取って `accepted=True` の取りこぼし (Status::cancelled に化ける) を構造的に防ぐ。
3. `engine.stop()` で取引ループを止める (同期 method)。
4. `servicer._teardown_live_components()` を呼ぶ ([server_grpc.py:252-271](python/engine/server_grpc.py#L252-L271)。**これは `GrpcDataEngineServer` のメソッド** であり module 関数ではない点に注意。`set_components` で登録した servicer instance 経由で呼ぶ。internally venue logout 等を扱う、live loop thread への join 含む)。
5. `event = server.stop(grace_seconds)` — 同期 gRPC server の `stop()` は `threading.Event` を返し、`event.wait(timeout=max(grace_seconds, 0) + 0.5)` で grace 完了を待つ (grace=0 でも 500ms の安全マージンだけ確保する。+1 秒固定にすると AppExit grace=0 経路で語義違反になるため最小化)。
6. `os._exit(0)`。

`set_components` の登録は **`serve()` 関数内で `server.start()` の直前に 1 回だけ呼ぶ** (= `server` / `engine` / `servicer` instance の 3 つすべてが構築済みになる箇所。具体的な行位置は Step 3 参照)。`GrpcDataEngineServer` のコンストラクタからは呼ばない (constructor 時点で `server` は未生成のため)。process_lifecycle 側はこれらを module-global に保持し、`start_shutdown()` から参照する。`_teardown_live_components` がメソッドのため `servicer` の登録が必須。live loop thread handle 自体は `_teardown_live_components` 内部で扱われるため、process_lifecycle 側で別管理しない。

**Shutdown RPC ハンドラは同期 `def`** ([server_grpc.py:GrpcDataEngineServer.Shutdown](python/engine/server_grpc.py)、Step 3 で追加)。中で次の判定を順に行う:

1. token 不一致 → `ShutdownResponse(accepted=False, error_code="INVALID_TOKEN")` を return (shutdown thread は起動しない)。
2. `process_lifecycle.start_shutdown(grace_seconds)` を呼ぶ。戻り値で「実際に thread を起動した / 既に shutdown 進行中だった」を区別できる必要があるため、`start_shutdown` は `bool` を返す API にする (`True` = この呼び出しで起動した、`False` = 既に進行中)。
3. `False` のとき → `ShutdownResponse(accepted=False, error_code="ALREADY_SHUTTING_DOWN")` を return。
4. `True` のとき → `ShutdownResponse(accepted=True, error_code="")` を return。

`start_shutdown` は内部で `threading.Thread(target=_shutdown_thread_main, args=(grace,), daemon=True).start()` を 1 回だけ実行 (多重呼び出しは無視するが戻り値で識別可能)。

クライアント (Bevy) はレスポンスを受けた後 deadline ベースで subprocess の exit を待つ。Rust 標準ライブラリの `std::process::Child` には timeout 付き wait がないため、本計画では **`wait-timeout` crate (`Child::wait_timeout(...)`) を採用** する (依存追加は Cargo.toml に 1 行)。代替として `try_wait()` を 100ms 間隔でループしても良いが、`wait-timeout` の方が記述が短く crate 利用は project の方針 (`tokio`/`tonic` 既に依存) と整合する。timeout 超過なら `Child::kill()` でフォールバック。**AppExit 経路ではこの wait を main thread で同期実行する** (`Child` を別 thread に move して Bevy 即 exit にすると、OS の process 終了で thread も道連れになり cleanup の完走が保証されないため、C-8 参照)。SIGINT 経路は signal handler が同じ `start_shutdown()` を呼んで即 return する (signal handler 内では複雑な処理を避け、thread spawn だけ)。

### C-7. クラッシュループカウンタとの統合

§3.8.6 のクラッシュループカウンタ (60 秒で 3 回 → CRASH_LOOP_DETECTED) は **`StartupFailed` も同じカウンタに数える**。「spawn → 5 秒以内に SERVING 来ない → Restart 押下 → また失敗」を 3 回繰り返したらユーザー観察を強制する。

### C-7b. Rust 側 async 実行所有者 (supervisor Tokio task)

`tonic::client::Channel` の RPC はすべて `async`。Bevy system は `fn` (同期) なので system 内で直接 `.await` できない。tick ごとに `tokio::spawn` で probe を発行すると次の事故が起きる:

- 200ms tick × 多重 in-flight RPC → backend の thread pool を食う。
- 古い probe レスポンスが遅延到着して新しい状態を巻き戻す (例: 500ms 前の SERVING が、その後の `Crashed` 判定後に到着して `Ready` に戻す)。
- main thread block を避けるために spawn しても、結果を ECS に戻す channel が tick ごとに作られると lifecycle 整合が崩れる。

このため、**probe / handshake / Health polling / crash 検知のすべては単一の supervisor Tokio task が所有する**:

- `src/backend_supervisor.rs::spawn_supervisor_task(rt: &tokio::runtime::Handle, lifecycle_tx: watch::Sender<BackendLifecycle>, ownership_tx: watch::Sender<BackendOwnership>, cmd_rx: mpsc::Receiver<SupervisorCommand>)` を Bevy 起動時 (`Startup` schedule) に **1 回だけ** spawn する。
- supervisor task の本体は state machine driver: 現在 state を保持し、自分の中で `tokio::time::sleep(Duration::from_millis(200))` で tick を駆動し、自分から `health_client.check(...).await` / `data_engine_client.get_state(...).await` を順次呼ぶ。in-flight RPC は同時に高々 1 本だけ。
- 状態が変わったら `lifecycle_tx.send(new_state)` で全 subscriber に通知。Bevy 側 system は `Res<BackendLifecycleHandle { rx: watch::Receiver<BackendLifecycle> }>` を read してフッター描画 / Tokio data task のゲーティング (C-2 で既述) に使うだけで、自分から RPC は打たない。
- Restart ボタン / Shutdown 指示は `SupervisorCommand::{Restart, Shutdown { grace_seconds: u32, reply_tx: Option<std::sync::mpsc::SyncSender<()>> }}` enum を `cmd_rx` 経由で supervisor に投げる (Bevy system → mpsc → supervisor)。`reply_tx` を `std::sync::mpsc::SyncSender` にしているのは AppExit 経路 (C-8) が **main thread (= Tokio runtime context 外)** で blocking 待ちするため (`tokio::sync::oneshot::Receiver::recv` は async API しか持たないので main thread からは block_on が必要になり実装が膨らむ。`std::sync::mpsc` の `recv_timeout` は同期 API で OS thread から直接呼べる)。Footer の手動 Shutdown ボタンなど ack が要らない経路は `None` を入れる。supervisor task は受信側でも `SyncSender::send(())` を直接呼べる (runtime context 不要)。supervisor が現在 state と整合する command なら受理して状態遷移し、整合しない command (Restart を Ready 中に押す等) は無視 + warn ログ + `reply_tx` があれば即 `send(())` で解放。
- stdout drain thread からの sentinel 検知も supervisor に届ける必要があるが、これは `cmd_rx` には乗せず **専用の `tokio::sync::mpsc::channel::<u16>(16)` (bounded)** で渡す。drain thread は素の `std::thread` であり async runtime context を持たないので、Sender 側は `sentinel_tx.blocking_send(port).ok()` で送る (Sender は `Send + Sync` で thread 跨ぎ可能、`blocking_send` は満杯時は最大 15 秒分待ってから drop、C-5 の timeout 仕様と整合)。supervisor は `tokio::select!` で `cmd_rx` と sentinel `Receiver` を別 arm として待ち、Spawning 中のみ sentinel を受理して Health.Check tick の最速トリガにする (Spawning 以外の state では `recv()` の結果を warn ログのみ出して破棄)。`SupervisorCommand` enum には sentinel variant を含めない (sender が ECS system ではなく素の `std::thread` であり、Bevy 経由で送れないため)。**`crossbeam-channel` crate は使わない** (`tokio::sync::mpsc` だけで完結し、`tokio::select!` への組み込みも素直になるため)。
- Tokio runtime は既存の `setup_backend_connection` ([src/main.rs:339](src/main.rs#L339) 周辺) が立てている `tokio::runtime::Runtime` を再利用するか、`#[tokio::main]` 化を避けるために `backend_supervisor` プラグインが自前で `Runtime::new()` する。既存実装の確認は Step 4 着手時に行い、重複起動を避ける。

これにより:
- ECS frame と RPC の同期取りが mpsc + watch の 2 経路に整理され、main thread block ゼロ。
- in-flight RPC は常に 1 本のみ。古いレスポンスが新状態を巻き戻すことは構造上起きない (supervisor 内で逐次 await するため)。
- crash 検知のループも supervisor task 内の同じループに統合され、別 system で並行に走らない。

Bevy 側 system (`health_check_polling_system` を rename → `backend_lifecycle_observer_system`) は **state watch の更新を観察してフッターのテキスト・色だけを書き換える純粋表示 system** に格下げする。RPC は撃たない。

### C-8. Job Object 配線の scope 切り出し

Job Object attach の本体実装は Phase 8 Step 4.6 の所管 (`§3.8.7`)。本計画 Step 4 では `BackendLifecycle::Spawning` が `Command::spawn` で子プロセスを生成するところまで作るが、**Job Object への attach 自体は配線しない**。よって本計画完了時点では:

- spawn 経路でも Bevy を `[×]` で閉じると Python が孤児プロセスとして残る (Bevy 側で `AppExit` event handler で **`BackendOwnership::own_process == true` のときのみ** `Shutdown` RPC を試み、無理なら `Child::kill()` を呼ぶフォールバックを入れる。`own_process=false` (attach 経路) の場合は何もせず Bevy だけ落とす — 開発者が別ターミナルで起動した外部 backend を巻き添え kill しないため (C-3 / ADR と整合)。Resource の `Drop` ではなく `AppExit` 経路にする理由は、`Drop` は Bevy のクラッシュ / `std::process::abort` 時に呼ばれないため。なお `AppExit` 自身が Bevy crash 時には発火しないので、いずれにせよ完全な解決は Job Object 配線 (Phase 8 Step 4.6) を待つ)。
- **AppExit cleanup は bounded synchronous wait で実行する** (detached thread に move する案は、Bevy が即 exit したら OS の process exit で thread も道連れになり `wait_timeout → kill()` の完走が保証されないため不採用。`std::process::Child` は Bevy parent が exit しても自動で reap されない — POSIX では init に親が引き継がれ、Windows では HANDLE がリリースされるだけで子は生き続ける)。
- **supervisor task との結合**: C-7b の通り gRPC clients と `Child` handle はすべて supervisor task が所有しているため、AppExit ハンドラ自身が直接 RPC / `wait_timeout` を呼ぶことはできない。代わりに次の経路を取る:
  - AppExit ハンドラは Bevy `AppExit` event を観測する system として実装し、`SupervisorCommand::Shutdown { grace_seconds: 0, reply_tx: Some(tx) }` を `cmd_rx` 経由で supervisor に送る (`tx` は `std::sync::mpsc::sync_channel::<()>(1)` の `SyncSender`、Receiver は main thread が保持して `recv_timeout(Duration::from_secs(2))` で blocking 待つ)。
  - supervisor task が `Shutdown` 受信時に: ① `Shutdown` RPC 発行 (deadline 1.0 秒)、② `Child::wait_timeout(Duration::from_millis(800))` を `spawn_blocking` 内で同期実行、③ 800ms 内に exit しなければ `Child::kill()` → `Child::wait_timeout(200ms)`、④ 完了したら `reply_tx.send(())` (`SyncSender::send` は同期 API、Tokio runtime context 不要)。supervisor task は `Stopped` 遷移後も `reply_tx.send` を済ませるまで生きる。
  - AppExit ハンドラ側は `reply_rx.recv_timeout(Duration::from_secs(2))` を **main thread で blocking** 待ち (`std::sync::mpsc::Receiver::recv_timeout` は同期 API)。2 秒で reply が来なければ諦めて Bevy exit (この場合 Child は孤児になるが、Bevy crash 時と同等の degraded path)。
- `grace_seconds` は AppExit 経路では 0 を使う (Bevy 側が短いタイムアウトで自前管理するため、Python 側に長い grace を投げて二重に待たない)。トータル最悪 **2.0 秒** main thread block。
- これでもユーザは「閉じたのに 2 秒固まる」体感を持つ。許容できない場合は Phase 8 Step 4.6 の Job Object 配線で OS による即時 cleanup に切り替わるため、それまでの暫定。本計画では **2 秒ブロックは受け入れる仕様** とする (orphan 残存 vs UX のトレードオフで前者を回避)。
- これは Phase 8 Step 4.6 (`backend_supervisor` Job Object 配線) で解消する前提。本計画完了→ Step 4.6 完了までの間は **開発者がタスクマネージャ (Windows: taskmgr / `taskkill /F /IM python.exe`、macOS: Activity Monitor / `pkill -f "python -m engine"`) で手 kill する運用**。Success Criteria でも「Bevy 閉じても python が残る」は attach 経路の確認のみで、spawn 経路の残存は仕様。

---

## 実装ステップ

### Step 1: Python 側 readiness signal (依存 0、最初に着手)

`python/engine/server_grpc.py:1167-1175`. **挿入位置は厳密に「現行 1169 行目 `server.start()` の直後、現行 1170 行目 `try:` の直前」とし、新規 1 行のみを追加する** (Step 3 で挿入する `set_components(...)` 呼び出しはこれより前 — 後述):

```python
server.add_insecure_port(f"127.0.0.1:{port}")
logging.info(f"Starting gRPC server on port {port}")
server.start()
# Readiness signal for Bevy backend_supervisor (plans/backend-startup-sync.md C-5)
print(f"GRPC_LISTENING port={port}", flush=True)
try:
    while True:
        time.sleep(86400)
except KeyboardInterrupt:
    ...  # Step 3 で書き換え (signal handler 経由で process_lifecycle.start_shutdown() を呼ぶ)
```

format `GRPC_LISTENING port=<port>` は **Rust/Python interface 契約** (C-5)。Step 1 単体では Python integration test を 1 本書く: `subprocess.Popen([sys.executable, "-m", "engine", "--token", "test-token", "--port", str(free_port)], stdout=PIPE)` で起動 → stdout から行を順次読み `re.fullmatch(r"GRPC_LISTENING port=\d+", line)` を満たす行が **1 行存在する** ことを assert (`logging.info()` 等の他行が混在しうるため line-stream を `re.fullmatch` でフィルタする)。format がレグレッションで壊れたら Step 4 着手前に検知できる。終了は `proc.terminate(); proc.wait(timeout=5)` で行う。

### Step 2: `Shutdown` RPC と `SERVICE_UNKNOWN` を proto に追加 (proto / stub 生成のみ)

`python/proto/engine.proto` に以下を追加 → stubs 再生成 (`make proto` 等のお決まりコマンド)。**本 Step は proto 追加と stub 再生成・相対 import 再パッチまでで止める**。`Shutdown` ハンドラ本体実装と subprocess 終了テストは `process_lifecycle.start_shutdown()` に依存するため Step 3 で行う (Step 2 単体ではハンドラを書けない):

1. C-6 の `Shutdown` rpc と `ShutdownRequest` / `ShutdownResponse` message。
2. **`HealthCheckResponse.ServingStatus` enum に `SERVICE_UNKNOWN = 3` を追加** (現状は `{UNKNOWN=0, SERVING=1, NOT_SERVING=2}` のみ、C-1 / Step 3 の Check 実装で必要)。append-only なので wire 互換は保たれる。Rust 側の生成 enum も自動更新される。

**現状の `python/engine/proto/engine_pb2_grpc.py:6` は既に相対 import `from . import engine_pb2 as engine__pb2` にパッチ済み**だが、`grpc_tools.protoc` で再生成すると絶対 import `import engine_pb2 as engine__pb2` に戻る ([§9 ADR](docs/plan/Phase%208%20-%20Live%20Venue%20and%20Market%20Data.md#L329-L333))。再生成のたびに相対 import に **再パッチ** すること (パッチ忘れ防止に `make proto` の中で sed/Python 後処理を入れるのが安全)。

### Step 3: `python/engine/process_lifecycle.py` に shutdown 集約 + Shutdown ハンドラ実装

Phase 8 Step 4.6 で予定されている `process_lifecycle.py` を **本計画で先に骨格作成**する (singleton / signal handler 全体は §3.8 本実装で埋める前提、本計画では shutdown 経路と Health.Check 連携のみ)。**現行は同期 gRPC server なので thread ベースで書く** (asyncio 依存なし):

```python
import os
import threading
import logging

_lock = threading.Lock()
_shutting_down: bool = False  # Health.Check が読む (lock 保護)
_shutdown_thread: threading.Thread | None = None
_components: dict = {}  # server / engine / servicer を登録 (lock 下で読み書き)

def set_components(
    *,
    server,
    engine,
    servicer,
) -> None:
    """server_grpc.serve() の server.start() 直前に 1 度だけ呼ぶ。
    live loop thread の join は servicer._teardown_live_components() が内部で処理する
    (server_grpc.py:252-271) ため、本モジュールでは live thread を別管理しない。"""
    with _lock:
        _components["server"] = server
        _components["engine"] = engine
        _components["servicer"] = servicer  # _teardown_live_components はメソッド

def is_shutting_down() -> bool:
    with _lock:
        return _shutting_down

def start_shutdown(grace_seconds: int = 3) -> bool:
    """Shutdown RPC ハンドラ / signal handler の両方から呼ぶ。
    戻り値: True = この呼び出しで shutdown thread を起動した。
            False = 既に shutdown 進行中で何もしなかった。
    多重呼び出しは構造的に無視 (lock で in-flight 判定)。"""
    global _shutdown_thread
    with _lock:
        if _shutdown_thread is not None:
            return False
        _shutdown_thread = threading.Thread(
            target=_shutdown_thread_main,
            args=(grace_seconds,),
            daemon=True,
            name="process_lifecycle_shutdown",
        )
        _shutdown_thread.start()
        return True

def _shutdown_thread_main(grace_seconds: int) -> None:
    global _shutting_down
    try:
        with _lock:
            _shutting_down = True   # Health.Check を NOT_SERVING へ
            components = dict(_components)  # スナップショット (以降は lock 外で読む)

        # Shutdown RPC レスポンスを wire に乗せきる猶予 (C-6 step 2)
        import time as _time
        _time.sleep(0.25)

        try:
            engine = components.get("engine")
            if engine is not None:
                engine.stop()       # 取引ループ停止 (同期)
        except Exception:
            logging.exception("engine.stop() failed during shutdown")

        try:
            servicer = components.get("servicer")
            if servicer is not None:
                servicer._teardown_live_components()  # メソッド (server_grpc.py:252)
        except Exception:
            logging.exception("_teardown_live_components() failed during shutdown")

        server = components.get("server")
        if server is not None:
            try:
                event = server.stop(grace_seconds)  # threading.Event を返す
                event.wait(timeout=max(grace_seconds, 0) + 0.5)
            except Exception:
                logging.exception("server.stop() failed during shutdown")
    finally:
        # どんな例外経路でも必ず exit。さもないと再 shutdown が永久に no-op になる。
        os._exit(0)
```

`HealthServicer.Check` ([server_grpc.py:273-276](python/engine/server_grpc.py#L273-L276)) を以下のように書き換える (現状は `request.service` を無視して常に SERVING を返す。C-1 の service フィルタと shutdown 中 NOT_SERVING を同時に入れる):

```python
def Check(self, request, context):
    # C-1: service フィルタ。"" (default) と "DataEngine" のみ受理
    # SERVICE_UNKNOWN は Step 2 で proto enum に追加 (= 3)
    if request.service not in ("", "DataEngine"):
        return engine_pb2.HealthCheckResponse(
            status=engine_pb2.HealthCheckResponse.SERVICE_UNKNOWN
        )
    if process_lifecycle.is_shutting_down():
        return engine_pb2.HealthCheckResponse(
            status=engine_pb2.HealthCheckResponse.NOT_SERVING
        )
    return engine_pb2.HealthCheckResponse(
        status=engine_pb2.HealthCheckResponse.SERVING
    )
```

これにより Bevy 側で `Crashed` (=clean death) と graceful shutdown 中 (=自発的に下りている、`NOT_SERVING` 受信から数秒は猶予) を区別できる。なお service フィルタはあくまで「全くの別 gRPC service が同 port を握っている」case の早期排除であり、**identity の最終確認は attach 経路の token 付き `GetState` handshake で行う** (C-1 / C-3)。`DataEngine` という service 名を SERVING で返してくる malicious / 偶発的な実装は handshake でしか弾けない。

`serve()` の `server.start()` 直前で `process_lifecycle.set_components(server=server, engine=engine, servicer=servicer)` を 1 度だけ呼ぶ。**挿入位置は厳密に「現行 1168 行目 `logging.info(...)` と現行 1169 行目 `server.start()` の間」**。`servicer` は `serve()` 内ですでに構築済み ([server_grpc.py:1155-1162](python/engine/server_grpc.py#L1155-L1162))。

live loop thread の管理は `_ensure_live_loop()` ([server_grpc.py:196-210](python/engine/server_grpc.py#L196-L210)) と `_teardown_live_components()` ([server_grpc.py:252-271](python/engine/server_grpc.py#L252-L271)) の閉じた範囲で完結させる (`process_lifecycle` 側に再エクスポートしない)。live venue 未使用 (backtest 専用起動) で live thread が存在しない case は、現行 `_teardown_live_components()` 先頭の `if self._live_runner is None and self._live_bridge is None: return` ([server_grpc.py:253-254](python/engine/server_grpc.py#L253-L254)) で no-op になることを確認済みなので追加防御は不要。

SIGINT / SIGBREAK ハンドラもこの `start_shutdown()` を呼ぶだけにし、`Shutdown` RPC と挙動を統一する (C-6)。signal handler は同期文脈で呼ばれ、内部は thread spawn のみなので signal-safety 上の制約を満たす。

**`Shutdown` RPC ハンドラの追加** (Step 2 で proto は生成済み): `GrpcDataEngineServer` に同期 `def Shutdown(self, request, context):` を追加し、C-6 の 4 段判定 (token 一致確認 / `start_shutdown()` 戻り値) に従って `ShutdownResponse` を組み立てて return する。具体的には: token 不一致 → `accepted=False, error_code="INVALID_TOKEN"`、`start_shutdown()=False` → `accepted=False, error_code="ALREADY_SHUTTING_DOWN"`、`start_shutdown()=True` → `accepted=True, error_code=""`。

**pytest** (Step 3 で書く、`start_shutdown()` 実装と Shutdown ハンドラがそろってから):
- `Shutdown(grace=0)` → subprocess が exit code 0 で 3 秒以内に終わることを `subprocess.Popen` ベースの integration test で確認。
- `grace > 0` のときは in-flight RPC (例: `GetState`) が grace 内で完了することを別ケースで確認。
- 二重 `Shutdown` 呼び出しで 1 回目が `accepted=True`、2 回目が `accepted=False, error_code="ALREADY_SHUTTING_DOWN"` を返すことを確認。

**既存 `except KeyboardInterrupt` ブロックの置き換え** (`server_grpc.py:1170-1175` の現行 `try: while True: time.sleep(86400) except KeyboardInterrupt: engine.stop(); server.stop(0)` を放置すると、Ctrl+C 時に `engine.stop()` が二重実行され `server.stop(0)` と shutdown thread の `server.stop(grace)` が並走する race が起きる):

1. `serve()` の `server.start()` の直前 (= `set_components(...)` 直後) で `signal.signal(signal.SIGINT, _on_signal)` を登録。Windows では `signal.SIGBREAK` も同じ handler に登録。`_on_signal` は `lambda *_: process_lifecycle.start_shutdown()` の薄いラッパ (thread spawn のみ、signal-safe)。
2. 現行の `try: while True: time.sleep(86400) except KeyboardInterrupt: engine.stop(); server.stop(0)` を **`server.wait_for_termination()` ブロッキング呼び出し 1 行に置き換える**。これは grpc 標準 API で、`server.stop()` が完了するまでブロックする。shutdown thread が最後に `os._exit(0)` を呼ぶため `wait_for_termination()` を抜けるパスは原則発火しないが、保険として直後で `return` する。
3. これにより shutdown 経路は SIGINT / Shutdown RPC / 内部エラーいずれも `process_lifecycle.start_shutdown() → _shutdown_thread_main()` の 1 経路に集約され、`engine.stop()` / `server.stop()` の二重呼び出しは構造的に排除される。

### Step 4: Rust `BackendLifecycle` Resource と probe 系 system

`src/backend_supervisor.rs` を新設し、

- `BackendLifecycle` enum (Disabled / NotStarted / ProbingExisting / Spawning / Ready / ShuttingDown / Stopped / Crashed / StartupFailed(&'static str)) — `ShuttingDown` / `Stopped` は graceful shutdown 経路用 (C-2 図 / Step 5)、`Crashed` は予期しない死亡用。`StartupFailed` の `&'static str` payload はエラーコード (C-2 参照)。
- **supervisor Tokio task** (C-7b で詳述、Startup schedule で **1 回だけ** spawn される) が、URL パース / TCP probe / Python spawn 判定 / `Health.Check` 200ms tick / 3 連続失敗による `Crashed` 判定 / `GetState` handshake / `NOT_SERVING` 由来の `ShuttingDown→Stopped` 分岐 / `SERVICE_UNKNOWN` の `BACKEND_IDENTITY_MISMATCH` 判定をすべて 1 タスク内で逐次 `await` する。in-flight RPC は常に 1 本以下。
- supervisor は内部で `tonic` clients を 1 セット保持 (`HealthClient`, `DataEngineClient`)。`Channel` は最初の TCP probe 成功後に `Channel::from_shared(url).connect().await` で構築し、`Crashed`/`Stopped` 遷移で drop して次回 Restart 時に再構築する。
- `spawn_python_backend_system` は **Bevy system ではなく supervisor 内の同期ヘルパ関数** `spawn_python_backend(...)` として実装する (`std::process::Command::spawn` は同期 API、supervisor task 内で `tokio::task::spawn_blocking` 経由で呼ぶ)。`BACKEND_AUTOSPAWN=0` のときは spawn を呼ばず `StartupFailed(BACKEND_NOT_REACHABLE)` を `lifecycle_tx.send(...)` する。
- stdout/stderr drain は `std::thread::spawn` で 2 本走らせる。**チャネルを 2 系統に分ける**: (a) sentinel 専用の `tokio::sync::mpsc::channel::<u16>(16)` (bounded、`blocking_send` で thread から送る)、(b) info/warn 行は **チャネルを経由せず drain thread 内で直接 `bevy::log::{info,warn}!("[backend] {}", line)` を呼ぶ** (thread-safe な subscriber 前提)。supervisor は sentinel 専用 `Receiver` + `cmd_rx` + Health tick を `tokio::select!` で待つ (どちらも `tokio::sync::mpsc::Receiver` なので追加ブリッジ不要)。
- ECS 側の `drain_backend_stdout_system` は **存在しない**。info / warn は drain thread が直接 `bevy::log` を叩き、supervisor channel には sentinel matched 行のみ流すため、Python 側の暴走ログで supervisor channel が詰まる経路は構造的に存在しない (チャネル容量 16 はあくまで sentinel + 数個の構造化シグナル想定)。
- `backend_lifecycle_observer_system` (Bevy system) は `Res<BackendLifecycleHandle>` の watch::Receiver から現在 state を read し、Footer のテキスト / 色を書き換える表示 system に徹する。RPC は撃たない。
  - **info/warn 経路が unbounded でも OOM しない理由**: drain thread は 1 行読み次第 `bevy::log::info!` を即同期呼び出ししてから次の `read_line()` に進むので、未処理キューを内部に持たない。`bevy::log` (tracing subscriber) 側のバッファ管理に従う。
  - **sentinel チャネルの送信仕様**: sentinel 行 (`^GRPC_LISTENING port=\d+$`) は drain thread が `Sender::blocking_send(port)` で送信。supervisor は Spawning 突入と同時に sentinel `Receiver::recv()` を `tokio::select!` の一 arm として消費開始しているため、capacity 16 が埋まる経路は構造的に存在しない (sentinel は 1 起動につき 1 行)。万一 supervisor 側が消費を始める前に send が来た場合でも 16 行までは内部 buffer で吸収する。`blocking_send` が長時間 block するのは supervisor が死んだ場合のみで、その場合は drain thread も Bevy プロセス終了まで block して構わない (cleanup は OS が行う)。
- `BackendStdoutLine(String)` Event で `^GRPC_LISTENING port=(\d+)$` regex マッチ行を **「Health.Check probe を開始して良い signal」** として扱う (Ready 遷移の独立トリガではない、C-1)。port 不一致は警告のみで `Health.Check` 経路で readiness 判定継続 (C-5)。spawn 経路の起動加速材であり、attach 経路では発火しない。
- log macro は `bevy::log::{info,warn,error}!` を使い、メッセージは `[backend] ...` prefix で統一 (C-5)

**`setup_backend_connection` Tokio task の接続ライフサイクル再設計** ([src/main.rs:339-453](src/main.rs#L339-L453)):

現行構造 (src/main.rs:371-453) は単一の Tokio task が次をすべて持つ:
- `DataEngineClient::connect()` retry loop (`backend_url` 直叩き、2 秒間隔で永久リトライ)
- 成功直後の `fire_list_instruments(...)` 1 回 (初回 instrument universe 取得、Phase 8 §3.5)
- main loop: `TransportCommand` (Pause/Resume 等) drain + `GetState` polling
- 失敗時の `BackendStatusUpdate::Error` push

これを `BackendLifecycle` watch と統合するには「数行挟む」では不足で、**接続ライフサイクル全体を Ready 駆動に書き換える**必要がある。再設計後の構造:

1. **`BackendLifecycle` watch を tokio task に配る**: `BackendLifecycle` を `Arc<tokio::sync::watch::Sender<BackendLifecycle>>` で wrap し、supervisor (送信側) / 本 task と Bevy 表示 system (受信側) に `Receiver` を配る。
2. **task は Ready 待ちでスタート**: task 本体冒頭に `lifecycle_rx.wait_for(|s| matches!(s, BackendLifecycle::Ready)).await` を置く。これで「Bevy 起動 → supervisor が probe/spawn を完了 → Ready」より前に `DataEngineClient::connect()` を打たない。**現行の `connect()` 永久リトライループは削除する** (Ready 到達後の connect は構造的に 1 発で成功するか、構造的バグなので即諦めて Crashed に倒したい)。
3. **Ready 到達 1 回ごとに connect + initial `ListInstruments` + main loop を 1 周回す**: task 本体は外側に「Ready を待つ → connect → initial ListInstruments → inner main loop → lifecycle が Ready から外れたら inner loop 脱出 → 1. へ戻る」という再接続ループを持つ。これにより:
   - 初回 `ListInstruments` (`fire_list_instruments(..., TickersSource::ReplayCatalogFallback, ...)`) は **再接続のたびに必ず 1 回再発火**する (Restart 後に instrument universe が空のままになる事故を防ぐ)。
   - `client` は inner loop 内のローカル変数で、再接続のたびに新しい `DataEngineClient` instance を握る (Restart 後の token / port 変更にも対応)。
4. **inner main loop での lifecycle 監視**: 既存の `loop { transport_rx.try_recv() ループ; get_state(); sleep(interval); }` を `tokio::select!` 化し、`lifecycle_rx.changed()` を別 arm として待つ。`Crashed` / `ShuttingDown` / `Stopped` / `StartupFailed` のいずれかに遷移したら inner loop を break して外側ループに戻る (= 次の Ready を待つ)。
5. **Ready 前の transport command の扱い**: `TransportCommandSender` は Bevy resource として常時存在する (現行通り)。Ready 前 (= inner loop に入っていない時間帯) に push された command は `transport_rx` に積まれるが、再接続後の inner loop 突入時に最新の command だけ drain する (古い Pause/Resume を順次再生すると意味が壊れるため、`while let Ok(_) = transport_rx.try_recv() {}` で先頭以外を捨ててから 1 個だけ処理する、もしくは「Ready 前は UI 側で Pause/Resume ボタンを disabled にする」かの 2 択。**本計画では後者を採用** — Footer / Strategy Editor 側で `backend_lifecycle != Ready` のとき transport ボタンを disabled にする (`backend_lifecycle_observer_system` が既に lifecycle を読んでいるので追加コストなし)。これにより `transport_rx` に古い command が積まれる経路自体を塞ぐ)。
6. **status push の整理**: `BackendStatusUpdate::Connected / Running / Error` は **lifecycle watch に責務移管**する (Footer 表示は `BackendLifecycleHandle` 経由に統一)。`setup_backend_connection` task が `BackendStatusUpdate` を送るのは廃止し、`StatusUpdateChannel` は instrument universe / venue state / execution mode の push 専用に限定する。Footer 描画 system は lifecycle watch と StatusUpdateChannel 両方を読む。

これにより「Ready 前の偽陽性 connect 失敗エラーが Footer に出続ける」「Restart 後に instrument universe が空のまま」「Restart 後に古い transport command が再生される」という 3 つの事故を構造的に塞ぐ。

`backend_update_system` ([src/trading.rs:282](src/trading.rs#L282)) は不変、channel drain のみ。

cargo test:
- `BackendLifecycle` 遷移を mock TcpListener + mock Command で deterministic に検証 (TCP listen を別 thread で持ち、connect 成功 → ProbingExisting → Ready)。
- stdout sentinel parser の golden test (`^GRPC_LISTENING port=(\d+)$` がマッチ / 不一致行を弾く / port 不一致を error log で検知)。
- stdout sentinel を受信しても **Health.Check SERVING が未到達なら Ready に遷移しない** ことの unit test (sentinel = probe trigger であり Ready trigger ではない)。
- `BACKEND_AUTOSPAWN=0` 時に Spawning に入らず `StartupFailed(BACKEND_NOT_REACHABLE)` へ遷移することを確認。
- `BACKEND_URL=https://...` / port 欠落で `StartupFailed(BACKEND_URL_INVALID)` を返すことを確認。
- Health.Check が `SERVICE_UNKNOWN` を返したら `StartupFailed(BACKEND_IDENTITY_MISMATCH)` へ遷移することを mock server で確認。
- attach 経路で `GetState` が `UNAUTHENTICATED` のとき `StartupFailed(BACKEND_TOKEN_MISMATCH)` へ遷移することを mock で確認。
- spawn 経路で Health.Check SERVING を返すが `GetState` が `UNIMPLEMENTED` を返す mock backend を立て、`StartupFailed(BACKEND_SERVICER_MISSING)` へ遷移することを確認 (DataEngine servicer 登録漏れの検知)。
- attach 経路 (`own_process=false`) で `AppExit` が発火しても `Shutdown` RPC を撃たないことを spy で確認 (外部 backend 巻き添え kill 防止のリグレッションテスト)。
- watch channel 統合: Bevy 側で `Ready` を送る前は Tokio task が GetState ループに入らないことを mock で確認。
- attach 経路の Health.Check リトライ: mock server を 500ms 遅延で SERVING にし、`ProbingExisting` が 2 秒 budget 内で `Ready` に遷移することを確認 (即 StartupFailed にならない)。
- supervisor task の tick は `tokio::time::sleep` 駆動 (C-7b) なので、`tokio::time::pause()` + `advance(Duration::from_millis(200))` で deterministic に Health.Check が呼ばれることを確認 (ECS frame 駆動ではないため `Time<Real>` mock は不要)。

### Step 5: supervisor task 内の crash / graceful-shutdown 判定ロジック

§3.8.5 の「`GetState` 60Hz polling に 200ms × 3 deadline」は **`Health.Check` 経路に移し替え**、判定の所有者を C-7b の supervisor Tokio task に置く (GetState は payload が大きく失敗ノイズが多いため、Health.Check の方が判定が綺麗)。`setup_backend_connection` 側 Tokio task の GetState 60Hz polling は引き続き state 取得用途で残すが、crash 判定責務からは外す (= 失敗してもエラートーストを出さず、watch channel の lifecycle 観察に従って中断/再開する)。Bevy ECS 側に独立した crash 検知 system は **作らない** (C-7b と整合)。

> **Phase 8 §3.8.5 への仕様変更**: 本計画は §3.8.5 の判定根拠を `GetState` から `Health.Check` に **書き換える**。逆輸入時に §3.8.5 の対応箇所を更新する TODO (Non-Goals 末尾参照)。

Step 3 で導入する `_shutting_down` フラグにより `Health.Check` が `NOT_SERVING` を返している間は **`Crashed` に遷移しない** (graceful shutdown 中なので)。状態遷移は C-2 の図に従い、`NOT_SERVING` を初めて受信した時点で `Ready → ShuttingDown`、`NOT_SERVING` 連続 30 回 (200ms × 30 = ~6 秒) または subprocess exit (exit_code=0) で `ShuttingDown → Stopped`。`Stopped` は **Crashed loop カウンタには加算しない** ([Restart Backend] は押下可能、無制限に押せる)。

`BACKEND_CRASHED` と `BACKEND_STARTUP_TIMEOUT` を **別エラーコード・別トースト文言** にし、Footer の `[Restart Backend]` ボタンは両方から共通遷移できるようにする。

### Step 6: 既存 backend attach の E2E 検証

`BACKEND_AUTOSPAWN` を default (`1`) のまま、`python -m engine` を別ターミナルで先に起動 → Bevy を起動 → `ProbingExisting → Ready` に直接遷移し、`tasklist /FI "IMAGENAME eq python.exe"` で python が **1 プロセスのみ** (Bevy が 2 個目を spawn していない) を確認。Bevy を [×] で閉じても python は生き残ることを確認 (attach 経路は Job Object に入れないため)。

加えて `BACKEND_AUTOSPAWN=0` で外部 backend なしの起動 → `StartupFailed(BACKEND_NOT_REACHABLE)` トーストが表示され、`[Restart Backend]` ボタンが disabled (spawn 経路無効化のため Restart 不可、ユーザは env を直して再起動する) になることを確認。

---

## Success Criteria

- Bevy 起動から **15 秒以内** に Footer の backend status が `Ready` (緑) に到達する (Windows 冷キャッシュ環境 / `python.exe` cold start を許容した値。実機ホット状態では 3-5 秒で到達することを期待。`taskmgr` で python.exe が立ち上がってから `Health.Check` SERVING までの経過を計測)。attach 経路 (外部 backend が立っている) では 2 秒以内。
- 起動中の数秒間に `BACKEND_CRASHED` トーストが **一度も出ない** (false positive ゼロ)。`bevy::log` 経由のログに `[backend] transition: NotStarted → Spawning → Ready` が 1 回のみ記録される。
- `BACKEND_AUTOSPAWN=1` で `python -m engine` を先に起動 → Bevy 起動すると `ProbingExisting → Ready` に直接遷移し、python.exe が 1 プロセスのみ。Bevy を閉じても python が残る。
- `PYTHON_BIN=` 未設定でも `.venv` 内の python が自動で選ばれる (Windows / macOS 両方で確認)。
- Python 側を `taskkill /F /PID <pid>` で殺すと、1 秒以内に Footer が `Crashed` に遷移し (Health.Check 200ms 間隔 × 3 連続失敗 = ~600ms 判定 + UI 反映)、`[Restart Backend]` ボタンが押下可能になる。押下で `Spawning → Ready` に 15 秒以内に戻る (C-1 budget と同じ)。
- 起動失敗 (例: port を別プロセスが掴んでいる) は `BACKEND_STARTUP_TIMEOUT` トーストで通知され、`BACKEND_CRASHED` とは別文言。
- 起動失敗 3 回連続 (60 秒以内) で `CRASH_LOOP_DETECTED` に入り `[Restart Backend]` が disabled になる (§3.8.6 と同カウンタを共有)。
- `Shutdown` RPC を gRPC client から叩くと、`ShutdownResponse(accepted=True)` が即座に返り、subprocess が exit code 0 で `grace_seconds + 2s` 以内に終了し、`tasklist` から消える。SIGINT 経路と挙動が同一 (process_lifecycle.start_shutdown() に集約、shutdown は別 thread で `_teardown_live_components` → `server.stop(grace).wait(...)` の順で同期実行)。shutdown 中の `Health.Check(service="DataEngine")` は `NOT_SERVING` を返し、`Health.Check(service="OtherService")` は `SERVICE_UNKNOWN` を返す。
- stdout に `GRPC_LISTENING port=<env で解決された port>` が **起動ごとに 1 回だけ** 出力される (重複 print なし)。format は正規表現 `^GRPC_LISTENING port=\d+$` にマッチする。
- pipe バッファ満杯による Python ハング無し: Python 側で 10MB 相当のログを `print()` 連打しても Bevy 側が drain 続け、subprocess が block しない (`time.sleep(0)` で確認可能な仕掛けを test fixture に入れる)。

---

## ADR

### ADR: Readiness は `Health.Check(service="DataEngine")` で統一、stdout sentinel は probe 開始の合図に降格

- TCP connect 単独は「bind したが servicer 未登録」を区別できない。
- stdout sentinel 単独で Ready とすると、attach 経路 (sentinel なし) と spawn 経路 (sentinel あり) の Ready 判定ロジックが分岐し、test カバレッジが二重化する。
- さらに sentinel は server.start() の直後を保証するだけで、Python 側で `add_DataEngineServicer_to_server()` を忘れる順序ミスを検知できない (`Health.Check` の service フィルタはこの「`DataEngine` servicer 未登録だが `Health` だけ登録」case を **直接は** 検知できないが、続く `GetState` handshake で `UNIMPLEMENTED` が返るため最終 Ready 判定では弾ける)。
- よって最終 Ready は: **Health.Check SERVING + token 付き GetState 1 回 OK** を spawn 経路 / attach 経路ともに Ready 条件にする。spawn 経路は token 一致が構造的に保証されているため `UNAUTHENTICATED` は出ないはずだが、`add_DataEngineServicer_to_server()` の呼び忘れ・proto 生成ミスを検知するために `GetState` を必須化する (`UNIMPLEMENTED` → `BACKEND_SERVICER_MISSING`)。attach 経路では加えて token 不一致 (`UNAUTHENTICATED` → `BACKEND_TOKEN_MISMATCH`) も区別する。sentinel は spawn 経路で「Health.Check probe を始めて良い」signal として補助的に使う。

### ADR: 既存 backend に attach した場合は Job Object に入れない

- 開発者が別ターミナルで起動した backend を Bevy 終了で巻き添え kill するのは事故。
- Job Object attach は Bevy が spawn した場合のみ。attach 経路は OS 側で reap されないことを ADR に明記し、§3.8.9 の「独立起動経路の責務はユーザー」と整合させる。

### ADR: Phase 8 §3.8.4 の二択を **Shutdown RPC 採用側に確定** する

§3.8.4 ([:1216](docs/plan/Phase%208%20-%20Live%20Venue%20and%20Market%20Data.md#L1216)) は「Shutdown RPC を新設するか、SIGINT/SIGBREAK 経路に一本化するか」を両論併記のまま残している。本計画は **Shutdown RPC 側で確定** させる。理由:

1. **attach 経路では SIGINT 一本化が成立しない**: C-3 の attach 経路では Bevy は parent process ではない。`Child::kill()` が握る handle が無く、`GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT)` も同じ console group の子にしか効かない。`TerminateProcess` は OS による強制 kill で graceful ではない。spawn 経路と attach 経路で shutdown 手段が分岐すると、`process_lifecycle.start_shutdown()` を通る保証が片方で崩れる。
2. **token 認証**: gRPC token 認証が効くので、ローカル `:19876` に偶発的に繋がる他プロセスが backend を勝手に殺せない。SIGINT に認証は無い。
3. **cross-host 余地**: Phase 9 以降で backend を別ホストに分離する余地が残る。SIGINT は cross-host に届かない。

SIGINT/SIGBREAK ハンドラは `Ctrl+C` / 非 `/F` の `taskkill` 受信路として残し、内部で同じ `process_lifecycle.start_shutdown()` を呼ぶ。これは「両方実装」ではなく「役割を分けた 1 経路ずつ」。

### ADR: `Shutdown` RPC と SIGINT は同じ `process_lifecycle.start_shutdown()` を呼ぶ

- 2 経路でロジックが分岐すると、graceful shutdown 順序 (§3.8.4 の VenueLogout → live loop teardown → server.stop) が片方で壊れる事故が起きる。`start_shutdown()` → `_shutdown_thread_main()` に集約することで Phase 9 の発注経路追加時にも単一フックで対応できる。
- 現行は同期 gRPC server なので、shutdown は asyncio task ではなく **別 thread** で実行する。Shutdown RPC ハンドラ (同期 `def`) は thread を spawn して即 `accepted=True` を return する (C-6)。これで自分自身の RPC レスポンスを返す前に `server.stop()` を呼んでしまう事故を避ける。

### ADR: shutdown は asyncio ではなく thread ベースで書く

現行 [server_grpc.py:1154](python/engine/server_grpc.py#L1154) は `grpc.server(futures.ThreadPoolExecutor(...))` の **同期 gRPC server**。RPC ハンドラはすべて `def` (非 async)。live loop は別 thread で asyncio loop を回しているハイブリッド構造 ([_ensure_live_loop()](python/engine/server_grpc.py#L200-L210))。

このため shutdown 工程を asyncio (`asyncio.all_tasks()`, `await server.stop()`) で書くと **そもそも main thread に event loop が無い** ので動かない。本計画では shutdown を `threading.Thread` で実行し、内部は同期 API のみ使う:

- `server.stop(grace)` は `threading.Event` を返すので `event.wait(timeout=...)` で grace 完了を待つ。
- `_teardown_live_components()` ([server_grpc.py:252-271](python/engine/server_grpc.py#L252-L271)) は既存関数で、内部で live loop thread への join やキャンセルを扱う。本計画ではこれを再利用する。
- live loop 内で動いている asyncio task の cancel は `_teardown_live_components()` の責務として既に存在するので、shutdown_thread からは関数を呼ぶだけで済む。

将来 Phase 9 以降で `grpc.aio` に移行する場合は本 ADR を見直し、`start_shutdown()` を `asyncio.create_task()` ベースに書き直す。

### ADR: 起動失敗とクラッシュをエラーコードで分ける

- ユーザー目線で「立ち上がらなかった」と「動いていたが死んだ」は対処法が違う (前者は env / venv / port を疑う、後者はログを見る)。
- 両方を `BACKEND_CRASHED` に潰すと、起動時の偽陽性で警告疲れが起き crash 検知の信用が落ちる。

### ADR: stdout sentinel は MVP では plain text で固定

`GRPC_LISTENING port=<n>` の plain text 形式を採用。NDJSON (`{"event":"ready","port":..,"pid":..}`) への拡張は Phase 9 で IPC が増えたタイミングで再検討。MVP では「Bevy 側 parser が 1 個の正規表現で済む」「`flush=True` だけで担保できる」を優先。

### ADR: `BACKEND_AUTOSPAWN=0` 時の attach 失敗は `StartupFailed` に倒す

env を尊重して `NotStarted` のまま「Connect Backend」ボタンを出す案もあるが、

- CI / e2e は外部 backend を別 host の `BACKEND_URL=` で渡す運用に統一できる (この場合 attach probe が成功するため `StartupFailed` には落ちない)。
- 開発者の利便より、「`BACKEND_AUTOSPAWN=0` は明示的に Bevy 側の起動責務を切り離した」という env のセマンティクスを優先したほうが意図が読みやすい。
- `[Restart Backend]` は disabled (spawn 経路無効のため意味がない)、ユーザは env を直して Bevy を再起動する運用。

---

## Non-Goals

- Phase 9 で実装予定の **idle gRPC timeout / 独立起動 backend の自己 shutdown** (§3.8.9 の Phase 9 繰越事項) は本計画のスコープ外。
- Phase 8 §3.8.6 の **自動再起動 (watchdog)** も Phase 9 以降。本計画は手動 Restart 経路のみを整える。
- **Footer の `[Restart Backend]` ボタン UI 自体の実装** (描画・押下処理・disabled 表示) は Phase 8 §3.8.6 の所管。本計画はそのボタンが押されたときの **`BackendLifecycle` 状態遷移とエラーコード分岐** だけを定義し、ボタン自体が存在することを前提とする。§3.8.6 がボタンを描く前に本計画が先行マージされた場合、Restart 経路は gRPC client / CLI からの手動 RPC キックで代用する (Success Criteria の検証も同様)。
- **Named Mutex / `engine.pid` / Job Object 本体実装** は Phase 8 Step 4.6 の所管。本計画は `process_lifecycle.py` の骨格と `set_components()` / `start_shutdown()` / `_shutdown_thread_main()` / `is_shutting_down()` だけを先取りで作る。本計画完了時点では spawn 経路でも Bevy クラッシュ時に Python が孤児プロセスとして残る (C-8)。
- credentials masking / login_dialog_runner の起動シーケンスは `plans/phase8-venue-menu-login.md` の所管。

## 逆輸入時 TODO (Phase 8 本計画への反映)

本計画を Phase 8 §3.8.11 として逆輸入する際の、§3.8 既存記述の更新事項:

- **§3.8.4 ([:1216](docs/plan/Phase%208%20-%20Live%20Venue%20and%20Market%20Data.md#L1216))**: 「修正: Shutdown RPC を追加するか、SIGINT/SIGBREAK 経路に一本化するか」の両論併記を削除し、本計画 ADR の決定 (Shutdown RPC 採用 + SIGINT は副次経路として残す) に書き換える。
- **§3.8.5 ([:1224](docs/plan/Phase%208%20-%20Live%20Venue%20and%20Market%20Data.md#L1224))**: crash 検知の判定根拠を `GetState` polling から `Health.Check` polling に変更 (本計画 Step 5)。`GetState` は引き続き 60Hz で叩くが crash 判定責務からは外す。
- **Step 2 proto 追加リスト ([:1335](docs/plan/Phase%208%20-%20Live%20Venue%20and%20Market%20Data.md#L1335))**: `Shutdown` RPC を追加。
- **Step 4.6 ([:1359-1369](docs/plan/Phase%208%20-%20Live%20Venue%20and%20Market%20Data.md#L1359-L1369))**: `BackendLifecycle` 状態機械 / readiness probe / stdout drain / `BACKEND_AUTOSPAWN` / Python interpreter 解決ルールを §3.8.11 から取り込む。Job Object attach は Step 4.6 で本実装。
