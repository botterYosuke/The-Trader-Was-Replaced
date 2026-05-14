# Phase 8: Live Venue & Market Data — Implementation Plan

[Tranceparent Headless Replay](./Tranceparent%20Headless%20Replay.md) Phase 8 を具体化する。Phase 6/7 で完成した **Replay 系統**（`replay_runner.py` + Replay State Machine + Snapshot Reducer + Bevy UI）を一切壊さずに、その横に **Live 系統** を新設し、実取引会場（Tachibana e支店 / kabu ステーション）からの認証・銘柄メタデータ・マーケットデータ購読までを headless backend に取り込む。注文・口座同期は Phase 9、Replay→Live のストラテジー昇格は Phase 10 で扱う。

## Goals

1. **Venue Login**: Tachibana / kabu ステーション API への認証フローを headless backend に実装。Rust UI からは資格情報を直接持たせず、`.env` / OS Credential Store / 環境変数経由で backend が解決する。
2. **Mode Mutual Exclusion**: `ReplayState` と `VenueState` を排他に管理し、Replay 実行中の venue login と、Live 接続中の `LoadReplayData` を **構造的に拒否**する。
3. **Ticker Metadata Sync**: 認証後に銘柄マスタを取得し、Nautilus `Instrument` へ変換、`InstrumentId` を Replay 側と同じ `<code>.TSE` 形式に正規化。
4. **Live Market Data Subscription**: 選択銘柄の `price` / `trades` / `depth` (10 段板) を購読し、既存の Snapshot Reducer に流して `TradingState` を 60Hz で更新する。Replay と同じ DTO を再利用するため UI 側コードはほぼ無改修。
5. **UI フィードバック**: Footer / Sidebar に Venue 接続状態・銘柄ロード進捗・購読中シンボル数を表示。Bevy UI 側は新しい floating window を追加せず、既存パネル（Kline / Ladder）を Live モードでも描画する。

## Non-Goals

- 実発注・約定処理・口座残高同期は **Phase 9** で扱う。本フェーズはあくまで **read-only な市場接続** に留める。
- Replay で動いたストラテジーをそのまま Live で起動する仕組み（Promote to Live）は **Phase 10**。
- 複数 Venue の同時接続（Tachibana と kabu を並列に張る）は Non-Goal。一度に接続するのは 1 Venue のみ。
- Tick データの永続化・録画は Non-Goal。Live で流れたバーは UI が表示するだけで catalog には書き戻さない。
- TWS / IBKR / FX 業者など、Tachibana / kabu 以外のアダプタ実装は Non-Goal。ただし `LiveVenueAdapter` インターフェイスは拡張可能な形にする。

---

## 0. Feature Inventory / バックエンド機能一覧

Phase 8 で backend に追加される責務を網羅列挙する。各項目は §3 以降の詳細設計に対応する。

### 0.1 Venue 認証

- `VenueLogin(venue, credentials_source)` — `credentials_source` は `"env"` / `"keyring"` / `"file"` の 3 種。Rust UI からは平文資格情報を渡さない。
- `VenueLogout()` — セッションを閉じてキャッシュを破棄。
- `GetVenueState()` — 現在の `VenueState`（DISCONNECTED / AUTHENTICATING / CONNECTED / SUBSCRIBED / RECONNECTING / ERROR）。
- 自動再接続: ネットワーク切断時に最大 3 回の指数バックオフ（1s / 4s / 16s）でリトライ。失敗で `ERROR` 遷移。

### 0.2 銘柄メタデータ

- `ListInstruments(venue, filter)` — 取得済みの全銘柄を返す（コード・名称・呼値単位・売買単位・市場区分）。
- `GetInstrument(instrument_id)` — 1 銘柄の詳細。
- 認証直後にバックグラウンドで全銘柄をフェッチし、Nautilus `Instrument` に変換してメモリキャッシュ。永続化は `dirs::cache_dir()/the-trader-was-replaced/instruments/<venue>.parquet` に書き戻す（日次更新）。

### 0.3 マーケットデータ購読

- `SubscribeMarketData(instrument_id, channels)` — `channels` は `["price", "trades", "depth"]` のサブセット。
- `UnsubscribeMarketData(instrument_id)` — 個別 unsub。
- 上限: 同時購読 50 銘柄（kabu PUSH の制限に揃える）。超過時は明示的に `SUBSCRIPTION_LIMIT_EXCEEDED` で reject。
- 内部で venue 固有の WebSocket / PUSH を一本に集約し、`LiveEventBus`（asyncio Queue）で `nautilus_trader` の `DataEngine` に push。

### 0.4 状態同期

- 既存 `GetState` の `TradingState` に `mode: "REPLAY" | "LIVE" | "IDLE"` を追加。Live モード時は `replay_state` を `None` にし、代わりに `venue_state` を返す。
- 既存 `GetPortfolio` は Phase 8 では Live 側からは常に空ポートフォリオを返す（Phase 9 で口座連携）。

### 0.5 ExecutionMode と並列稼働

「Replay engine の稼働」と「Venue 認証 / market data 購読」は **同時並行で動かせる** 設計に変更（旧計画の `ModeManager` 排他案は撤回）。理由は本フェーズ末尾「ADR: 認証と Execution は排他しない」参照。

- `ExecutionMode: Replay | LiveManual | LiveAuto` — 以下の **4 つ** を決める明示的なユーザ選択。UI トグルで切替（§3.5.1）:
  1. **発注経路の宛先** — Replay simulator / Live venue 手動発注 / Live venue 自動発注
  2. **画面の主時間軸** (`current_time = replay_time` / `current_time = wall_clock`) — Footer 時刻表示・チャート末尾位置
  3. **時系列ウィンドウの表示判定** — モードの時間軸に一致しない時系列データは表示しない（混在禁止、§0.5.1）
  4. **UI_LAYOUT の保存先** — モードによって保存先が異なる（§0.5.2 参照）
- `ReplayState` / `VenueState` は独立に遷移し、互いに **排他しない**。具体的には:
  - Replay `RUNNING` 中に Venue `Login` 可能 → Sidebar の Tickers リストが Live venue universe で更新される（要望: 「Phase 7 のローカル固定銘柄一覧をログインしたら更新する」）
  - Venue `CONNECTED` 中に `LoadStrategy` / Replay `Run` 可能 → 並行してバックテストを走らせられる
- `ModeManager` の責務は **発注経路の安全性のみ** に縮約:
  - `ExecutionMode == LiveManual` または `LiveAuto` を選ぶには `VenueState >= CONNECTED` が必須
  - `ExecutionMode == Replay` を選ぶには `ReplayState >= LOADED` が必須
  - 違反は `EXECUTION_MODE_PRECONDITION` エラーで reject
- 「Replay と Live の二重発注事故」は ExecutionMode が排他であることで構造的に防ぐ。データの読み取り（market data 購読）と認証は二重制約しない

### 0.5.2 UI_LAYOUT の保存先（ExecutionMode 別）

**基本方針**: 戦略 `.py` と **同名の `.json` ファイル（サイドカー）** に保存する。`.py` + `.json` を 1 組の入力ファイルとして扱う。`.py` 本体は一切改変しない（sentinel block 方式は不採用）。

| ExecutionMode | UI_LAYOUT の保存先 | 理由 |
|---|---|---|
| **Replay** | `{strategy_name}.json`（`.py` と同じディレクトリ） | 戦略ファイルと 1:1 対応。バックテスト文脈（SCENARIO・コード）と UI 状態を同じ場所に置く |
| **Live Auto** (Phase 10) | `{strategy_name}.json`（`.py` と同じディレクトリ） | Replay と同方式。Promote to Live でそのまま `.py` + `.json` ペアを引き継ぐ |
| **Live Manual** | `cache_dir/the-trader-was-replaced/live_manual_layout.json` | 対応する戦略 `.py` が存在しない。ウィンドウ位置のみを standalone ファイルで保持 |

- 保存タイミング: ウィンドウ移動・リサイズのたびに変更イベント駆動で自動保存（`response.changed()` 相当）。非同期 I/O で UI フレームをブロックしない。
- `UiLayoutCache` Resource の保存先選択は `ExecutionModeRes` を参照:
  ```rust
  match mode {
      ExecutionMode::Replay | ExecutionMode::LiveAuto => {
          // {original_path stem}.json
          save_to_sidecar_json(strategy_path.with_extension("json"))
      }
      ExecutionMode::LiveManual => {
          save_to_json(cache_dir.join("live_manual_layout.json"))
      }
  }
  ```
- Live Auto（Phase 10）は本フェーズではスタブのみ（`LiveAuto` enum バリアントを定義。保存先ロジックは `Replay` と共通パスを使う）。

### 0.5.1 時系列データの時間軸ルール（混在禁止）

Replay モードは独自の `replay_time`（例: 2024-04-15 09:30 JST）で進行し、Live モードは wall clock（例: 2026-05-14 15:42 JST）で進行する。**両者を同じ画面に並べると「この値はいつのもの？」というユーザ誤解を生む**ため、ExecutionMode に応じて時系列ウィンドウの可視性とデータソースを切り替える:

**原則**: 時系列データ (price / depth / trades / fills / 約定履歴) は `ExecutionMode` の時間軸と一致するソースのみ表示する。
- `ExecutionMode == Replay`: Replay engine 由来のデータのみ表示。Live tick は流れていても画面には出さない
- `ExecutionMode == Live`: Live venue 由来のデータのみ表示。Replay engine の進行は backend で続いていても画面には出さない

**例外**: 銘柄一覧（静的メタデータ：銘柄コード・名称・呼値単位・売買単位）は時系列ではないため、**両モード共通で Live venue (Tachibana / kabu) から取得**する。Replay モードでも Sidebar に最新の Live universe が並ぶ。理由: J-Quants catalog は historical 断面のスナップショットで、最新の上場/廃止情報を持たない。ユーザの利便性のため、銘柄リストは常に最新の取引可能 universe を見せる。
- Venue 未ログイン時は Replay catalog（`ListInstruments(source="replay")`）にフォールバック
- Venue ログイン後は **Live universe で上書き**（Replay 時もこちらが正、Phase 7 の旧設計を改める）

**各ウィンドウの挙動**:

| ウィンドウ | Replay モードでの表示 | Live モードでの表示 |
|---|---|---|
| Footer 時刻 | `replay_time`（主） | wall clock（主） |
| KlineChartWindow | Replay engine の historical バー | Live aggregated バー |
| **LadderWindow** | **強制非表示**（depth は Live のみ、時間軸混在禁止） | Live depth 表示 |
| BuyingPower / Positions / Orders | Replay engine 由来（simulator のポジション） | Live venue 由来（実口座のポジション、Phase 9） |
| Sidebar 銘柄一覧 | **Live universe**（例外）または未ログイン時 Replay catalog | Live universe |
| Sidebar の「最新価格」列 | Replay engine の最新バー close | Live tick の最新 mid |

---

## 1. Architecture / 構成

### 1.1 Process Layout

```mermaid
flowchart TD
    UI["Bevy UI (Rust)<br/>src/trading.rs"]
    UI -->|gRPC :19876| GRPC

    subgraph PY["Python headless backend"]
      GRPC["server_grpc.py"]
      MODE["ModeManager"]
      REPLAY["replay_runner.py<br/>(Phase 6, unchanged)"]
      LIVE["live_runner.py [NEW]"]
      ADAPTER["LiveVenueAdapter [NEW]<br/>protocol"]
      TACHI["TachibanaAdapter [NEW]"]
      KABU["KabuStationAdapter [NEW]"]
      REDUCER["reducer.py<br/>(shared)"]
      STATE["TradingState"]

      GRPC --> MODE
      MODE --> REPLAY
      MODE --> LIVE
      LIVE --> ADAPTER
      ADAPTER --> TACHI
      ADAPTER --> KABU
      REPLAY --> REDUCER
      LIVE --> REDUCER
      REDUCER --> STATE
      GRPC -->|GetState/GetPortfolio| STATE
    end

    TACHI -->|HTTPS + WebSocket| TACHI_API["kabuka.e-shiten.jp"]
    KABU -->|REST + WebSocket| KABU_API["localhost:18080"]
```

### 1.2 State Machines

```text
ReplayStateMachine (Phase 6)
   IDLE → LOADED → RUNNING ⇄ PAUSED → STOPPING → IDLE

VenueStateMachine (Phase 8 [NEW])
   DISCONNECTED → AUTHENTICATING → CONNECTED → SUBSCRIBED
                                       ↑           ↓
                                   RECONNECTING ←──┘
                                       ↓
                                     ERROR → (manual reset) → DISCONNECTED
```

`ReplayStateMachine` と `VenueStateMachine` は **完全に独立** に動く。`ModeManager` は両者を観測しつつ、**ExecutionMode（発注経路）の前提条件だけ**を守る:

- `ExecutionMode == Live` ⇒ `VenueState >= CONNECTED` が必須（未ログインで Live モードに切替不可、`EXECUTION_MODE_PRECONDITION` で reject）
- `ExecutionMode == Replay` ⇒ `ReplayState >= LOADED` が必須（戦略未ロードで Replay モードに切替不可）
- ただし `Login` / `Logout` / `LoadStrategy` / `Unload` 自体は **どの ExecutionMode** でも、**どの相手側 state** でも実行可能。
- 発注 RPC（Phase 9 で追加予定）は ExecutionMode を見て宛先を決める。`Replay` モードなら必ず simulator、`Live` モードなら必ず venue、誤配は構造的に発生しない。

### 1.3 LiveVenueAdapter Protocol

```python
class LiveVenueAdapter(Protocol):
    venue_id: str  # "TACHIBANA" / "KABU"

    async def login(self, creds: VenueCredentials) -> None: ...
    async def logout(self) -> None: ...
    async def fetch_instruments(self) -> list[InstrumentRaw]: ...
    async def subscribe(self, instrument_id: InstrumentId, channels: set[Channel]) -> None: ...
    async def unsubscribe(self, instrument_id: InstrumentId) -> None: ...
    def events(self) -> AsyncIterator[LiveEvent]: ...   # price / trade / depth
```

- 各アダプタは asyncio タスクとして実行し、`events()` から `KlineUpdate` / `Trades` / `Depth` イベントを yield。
- 共通 reducer がそれを `TradingState` に畳み込むため、Replay と同じ UI コードで描画できる。
- 認証エラー（HTTP 401 / kabu の `4001005` 等）はアダプタが `VenueAuthError` に正規化し、上位の `VenueStateMachine` が `ERROR` 遷移を決める。

---

## 2. Venue 固有の取り扱い

> 詳細プロトコルは `.claude/skills/tachibana/SKILL.md` / `.claude/skills/kabusapi/SKILL.md` を参照すること。本計画書ではアーキテクチャ上の差分のみ記載する。

### 2.1 Tachibana (e支店)

- ベース URL: `https://kabuka.e-shiten.jp` (本番) / `https://demo-kabuka.e-shiten.jp` (検証)。
- 認証: `CLMAuthLoginRequest` → `sUrlRequest` / `sUrlMaster` / `sUrlPrice` / `sUrlEvent` / `sUrlEventWebSocket` の 5 つの仮想 URL を受領。各 RPC は `{virtual_url}?{JSON文字列}` 形式。
- 必須パラメータ: `p_no`（連番）/ `p_sd_date`（YYYY.MM.DD-HH:mm:ss.fff）/ `sJsonOfmt`。
- エンコーディング: Shift-JIS。`p_errno` と `sResultCode` の二段判定。
- マーケットデータ: EventWebSocket（区切り `\x01\x02\x03`）。
- 第二暗証番号が必須化されているため、`credentials_source` に追加フィールドを持たせる。
- 検証フロー: 認証 → `CLMEventDownload` でマスタ取得 → EventWebSocket で板/歩み値を購読。

### 2.2 kabuステーション (kabusapi)

> 詳細は `.claude/skills/kabusapi/SKILL.md` を参照。以下は Phase 8 計画上の含意のみ。

- **OS 制約**: kabuステーション本体が Windows GUI アプリのため **Windows 限定**。Linux/Mac/WSL では動かない（Wine 非対応）。CI で本物 API を叩く運用は不可。`pytest -m demo_kabu` は **`httpx-mock` のみで構成**し、live ジョブは設けない（skill 前提条件 §5）
- **ベース URL（リテラルは `kabusapi_url.py` の 1 箇所のみ）**:
  - 本番 `http://localhost:18080/kabusapi/` — `KABU_ALLOW_PROD=1` env が立っているときのみ Python URL builder が解禁
  - 検証 `http://localhost:18081/kabusapi/` — 既定。`DEV_KABU_PROD` 未設定 / `false` ならこちら
- **本体プロセス前提**: kabuステーション本体（Win GUI）が起動 → 本体に手動ログイン → 設定で「APIを利用する」✅ → APIPassword 設定、までを **ユーザーが手動で済ませる**。`live_runner` 起動時に `:18080` / `:18081` の TCP LISTEN を確認し、未 LISTEN なら `KABU_STATION_NOT_RUNNING` で reject。本体は早朝に強制ログアウトされる仕様のため、深夜 E2E では落ちる前提（skill S1）
- **認証**: `POST /token {"APIPassword": "..."}` → トークン取得 → 以降の全リクエストに `X-API-KEY` ヘッダ。Bearer ではない（skill R3）
- **トークン寿命**: 本体終了/ログアウト/別トークン発行で失効。**ファイルキャッシュは作らない**（毎起動 `/token` を叩き直す、skill S4）。Tachibana の session_cache 経路は kabu には無い
- **マーケットデータ**: WebSocket `ws://localhost:1808X/kabusapi/websocket`。UTF-8 JSON、認証ヘッダ不要（本体ログイン状態が前提）。受信専用（クライアントから何も送らない）
- **keepalive ping は無効化**: kabuStation は RFC 6455 準拠の PONG を返さないため、`ping_interval=None` を必ず指定し、`asyncio.wait_for(ws.recv(), timeout=3600)` で無メッセージハングを検出して再接続する（skill R8 / Issue #40）
- **銘柄登録 50 銘柄上限**: REST と PUSH の合算で 50 銘柄（skill R6）。`GET /board` も内部的に自動登録を発火するため、明示登録 + GET の両方を `kabusapi_register.RegisterSet` で集計する。51 銘柄目は `4002001` で reject。`SubscribeMarketData` 上限はここから来る
- **流量制限（明示）**: 発注系 5 req/s / 余力系 10 req/s / 情報系 10 req/s。`kabusapi_ratelimit.OrderBucket/WalletBucket/InfoBucket` の **token-bucket** で事前抑制する（skill R5）。`4002006` がサーバから返ったらバックオフリトライ
- **Symbol キー**: `<SymbolCode>@<Exchange>` 複合（例 `5401@1`、`1`=東証 / `3`=名証 / `5`=福証 / `6`=札証）。組立ては `kabusapi_url.symbol_key(symbol, exchange)` に集約（skill R4）
- **エラーコード**: HTTP status と body の `Code` フィールドの 2 段判定。代表値は `4001001` 未認証 / `4001003` APIPassword 不一致 / `4001005` トークン期限切れ（自動 1 回 retry）/ `4002001` 登録上限 / `4002006` 流量制限 / `4002008` 銘柄未登録（skill R7）
- **訂正 API なし**: kabuステーションには訂正専用エンドポイントが無く、「取消 → 新規発注」で実現する。Phase 9 で発注経路を作る際の前提として記録（立花とは違うので注意）

### 2.3 InstrumentId 正規化

- Replay 側は `<code>.TSE` 形式（例 `1301.TSE`）。Live でも同形式に揃える。
- Tachibana のマスタは 4 桁コード + 市場区分コード。市場区分 = 東証 のものを `.TSE` にマップ。他は `.OSE` / `.NSE` 等にする（MVP は TSE のみで OK、それ以外は warn ログを出して skip）。
- kabu のマスタは `Symbol@Exchange` 複合キーで持つが、UI 表示用の `InstrumentId` は同じ `<code>.TSE` / `.OSE` に正規化する。逆変換テーブル（`<code>.TSE` ↔ `(symbol, exchange=1)`）を `kabusapi_url.symbol_key` 周辺に置く。両 venue で同じ `InstrumentId` が出るため、UI 側は venue を意識せずに表示できる。

---

## 3. Tasks

### 3.1 Backend: ExecutionMode & State 拡張

- `python/engine/mode_manager.py` を新設。`ReplayStateMachine` / `VenueStateMachine` の両 owner を保持し、**ExecutionMode** の遷移ガードのみを担う（排他制御は持たない）。
- `TradingState` (`python/engine/models.py`) に以下を追加:
  - `execution_mode: Literal["Replay", "LiveManual", "LiveAuto"]` — 発注経路の宛先（既定 `"Replay"`）。`"LiveAuto"` は Phase 10 用のスタブとして定義しておく
  - `replay_state: Optional[str]` — Replay engine の状態（既存）
  - `venue_state: Optional[str]` (`DISCONNECTED` 等) — Venue 接続状態（独立）
  - `venue_id: Optional[str]`
  - `subscribed_instruments: list[str]`
- `core.py::get_current_state()` を更新。Replay 側と Venue 側を**並列で**返す。どちらかが NULL ということは無い（IDLE 等は値を持つ）。
- `LoadReplayData` / `StartEngine` / `VenueLogin` 系の Mode ガードは **撤廃**。Replay 中の Login も、Live 接続中の Replay Load も許可する。
- **新 RPC `SetExecutionMode(mode)`** を追加。`Replay` / `Live` の切替を明示的に行い、前提条件不足なら `EXECUTION_MODE_PRECONDITION` で reject。
- `ListInstruments` を **Phase 7 で導入予定の `source="replay"` に `source="live"` を追加**（既存 RPC を拡張）。Phase 7 で先に skeleton が作られている前提。

### 3.2 Backend: LiveVenueAdapter & 具象実装

venue 共通の抽象は `python/engine/live/` に置き、venue 固有のプロトコル実装は **`python/engine/exchanges/` 配下に venue 名で集約**する（tachibana skill `.claude/skills/tachibana/SKILL.md` の規約 R1 / F-L1 を踏襲。「立花プロトコル固有のヘルパーは Rust に書かない／Python は `exchanges/tachibana*.py` に集める」）。kabusapi も同方針で `exchanges/kabu*.py` に置き、Python に集約する。

```
python/engine/
├── live/                          # venue 非依存の枠組み
│   ├── __init__.py
│   ├── adapter.py                 # LiveVenueAdapter Protocol / LiveEvent
│   ├── state_machine.py           # VenueStateMachine
│   ├── event_bus.py               # adapter → reducer の asyncio Queue
│   ├── aggregator.py              # tick → bar 集約（Nautilus BarBuilder ラッパ）
│   ├── instrument_mapping.py      # venue 共通の InstrumentId 正規化（.TSE 等）
│   └── logging.py                 # secrets masking filter
└── exchanges/                     # venue 固有プロトコル（Rust 側に同等実装を作らない）
    ├── __init__.py
    ├── tachibana.py               # LiveVenueAdapter 実装（薄いラッパ）
    ├── tachibana_url.py           # build_request_url / build_event_url / func_replace_urlecnode (R2/R9)
    ├── tachibana_auth.py          # next_p_no / current_p_sd_date / check_response / 例外型 (R4/R6)
    ├── tachibana_codec.py         # Shift-JIS decode / ^A^B^C parse / 空配列 "" → [] (R7/R8)
    ├── tachibana_ws.py            # EventWebSocket クライアント (sUrlEventWebSocket)
    ├── tachibana_master.py        # CLMEventDownload マスタ取得
    ├── tachibana_file_store.py    # tachibana_session.json ファイルキャッシュ (R3 / S3)
    ├── tachibana_login_flow.py    # debug 専用 env 取込み + tkinter ダイアログ起動の橋渡し
    ├── kabu.py                    # LiveVenueAdapter 実装
    └── kabu_*.py                  # localhost:18080/18081 REST + WebSocket
```

認証情報の解決順（**いずれの経路も Rust → Python に資格情報を渡さない**。`VenueLogin` RPC は「ログイン開始」のトリガのみで、ペイロードに password を含めない）:

1. `credentials_source == "prompt"`（**既定**）⇒ Python プロセスが tkinter サブプロセスでログインウィンドウを開く（§3.2.1）
2. `credentials_source == "session_cache"` ⇒ Tachibana は `cache_dir/tachibana/tachibana_session.json` から仮想 URL 一式を復元（JST 当日付に限り有効、skill R3 / S3）。kabu は token 再取得が軽量なので session cache なし
3. `credentials_source == "env"` ⇒ **debug ビルドの Python のみ**が読む（release は無視）。env 名は **venue prefix 付きで統一**:
   - Tachibana: `DEV_TACHIBANA_USER_ID` / `DEV_TACHIBANA_PASSWORD` / `DEV_TACHIBANA_DEMO`（既定 `true`）
   - kabu: `DEV_KABU_API_PASSWORD` / `DEV_KABU_PROD`（既定 `false` = 検証 18081 を叩く）
   - **第二暗証番号 / 取引パスワードは env に置かない**（Tachibana skill F-H5、kabu skill R10 / S4）。後述 §3.2.1 参照
4. `credentials_source == "keyring"` / `"file"` は **採用しない**。Tachibana skill が `tachibana_session.json` ファイルキャッシュに集約しており、keyring も平文 file もこの方針と衝突する

本番接続のガード:
- Tachibana: `DEV_TACHIBANA_DEMO` 未設定 = demo 既定。本番 URL `https://kabuka.e-shiten.jp/e_api_v4r8/` への接続は **`TACHIBANA_ALLOW_PROD=1` env を併用したときのみ** Python URL builder が解禁する（Tachibana skill S2 / Q7）
- kabu: `DEV_KABU_PROD` 未設定 / `false` = 検証 18081 既定。本番 18080 への接続は **`KABU_ALLOW_PROD=1` env を併用したときのみ** Python URL builder が解禁する（kabu skill R1 / S3）。`DEV_KABU_PROD=true` 単体では検証ポートに落とす二重ガード

平文の資格情報は **絶対にログに出さない**。`logger` の `extra` フィルタで `password|token|api_key|p_pwd|sPassword|sSecondPassword|virtual_url|sUrl[A-Z]` を含むキーをマスクする helper を `live/logging.py` に置く（Tachibana skill R10）。仮想 URL もセッション秘密なのでマスク対象（`***` 化）。

### 3.2.1 Python 側のログインウィンドウ

- 実装: `tkinter`（Python 標準ライブラリ、追加依存ゼロ）の **サブプロセスヘルパー**として起動する。`python/engine/live/login_dialog_runner.py` がフロントエンドとなり、`python -m engine.live.login_dialog_runner --venue tachibana --env demo` で別プロセスを spawn、結果は stdout JSON で受け取る。
  - Tachibana の入力フィールド定義は **`python/engine/exchanges/tachibana_login_flow.py`** に集約（Rust 側に立花用ログイン UI を書かない方針、skill「Rust 側に置かないもの」を参照）。kabu の入力フィールド定義は `exchanges/kabu_login_flow.py` に置く
  - サブプロセス分離により Bevy/asyncio イベントループを `Tk.mainloop()` でブロックしない
- 表示タイミング: `VenueLogin(credentials_source="prompt")` 受信時、`server_grpc` ハンドラが `VenueState` を `AUTHENTICATING` に遷移させてから即座に "pending" を返す。サブプロセスの stdout JSON を asyncio で読み、完了時に `VenueState` を `CONNECTED` / `ERROR` に再遷移。UI は `GetVenueState` を polling して確認する（既存 60Hz polling パスを再利用）
- **入力フィールド**（venue 固有 / skill 準拠）:
  - **Tachibana**: ユーザー ID / パスワード / 環境（demo/prod 選択、prod 選択肢は `TACHIBANA_ALLOW_PROD=1` が立っていないとグレーアウト）。**第二暗証番号は収集しない**（skill F-H5: ログイン時には不要。発注時に Phase 9 の別 UI で取得しメモリ保持・idle forget タイマーで自動消去）
  - **kabu**: API パスワード / 環境（検証/本番 選択、本番選択肢は `KABU_ALLOW_PROD=1` が立っていないとグレーアウト）。kabuステーション本体プロセスの listening ポート（18080 / 18081）を読み取り専用で表示。未起動なら `KABU_STATION_NOT_RUNNING` を表示して [再確認] ボタンを出す。本体側で「APIを利用する」が OFF / 未ログインの場合は `4001001` / `4001003` を捕捉して原因別メッセージを表示
- セッション内キャッシュ:
  - **Tachibana**: ログイン成功時に `tachibana_file_store` が `cache_dir/tachibana/tachibana_session.json` に**仮想 URL 一式のみ**を保存（JST 当日付）。ユーザー ID / パスワードはディスクに書かない。次回起動時はこの session JSON を session_cache 経路で復元（skill S3）
  - **kabu**: `/token` で取得した X-API-KEY トークンのみ `live_runner` メモリ内に保持。**ディスクには一切書かない**（本体終了/ログアウトで失効するため永続化する価値が無い、kabu skill S4）。失効時 (`4001005`) は最大 1 回 retry で `/token` を再発行、それでも失敗なら `KabuTokenExpiredError` を上層に伝播してログイン UI を再表示
- ウィンドウは Bevy UI / asyncio loop と **完全に独立したサブプロセス**。Bevy が落ちても認証フローに影響なし、逆も同様
- headless 環境（DISPLAY 無し / Win32 GUI 無し）では `tkinter` の `Tk()` インスタンス化が失敗する。サブプロセスはこれを検知して `{"error_code": "NO_DISPLAY_AVAILABLE"}` を JSON で返す。`server_grpc` ハンドラはそれを `env` への切替を促すエラーメッセージにマップして UI に返す
- CI 上で立花ライブログインが必要なテストは **`pytest -m demo_tachibana`** で隔離し、GitHub Actions では **`workflow_dispatch` 限定**のジョブで実行する（Tachibana skill 前提条件 §4 / open-questions Q21）。PR/push トリガには載せない（閉局帯ヒットによる偽陰性回避）

### 3.3 Backend: live_runner.py

- `python/engine/live_runner.py` を新設。`replay_runner.py` と同じ位置付け（独立した asyncio 駆動ループ）。
- 責務:
  - `LiveVenueAdapter` を 1 つ保持し、`events()` を fan-out
  - 受信した `LiveEvent` を Nautilus `DataEngine` に inject（Replay と同じ msgbus トポロジを使う）
  - 結果として `reducer.py` がこれまで通り `TradingState` を更新
- Live 側でも `nautilus_trader` の `DataEngine` をホストする。ただし `TradingNode` (live execution) は **使わない**。発注経路を握らないため、誤って実発注しないことを構造的に担保する。

### 3.4 Backend: gRPC RPC 追加

`python/engine/proto/engine.proto` への追加:

```protobuf
service Engine {
  // ... existing replay RPCs ...
  // Phase 7 で追加済み: ListInstruments(source="replay")

  // Phase 8
  rpc VenueLogin (VenueLoginRequest) returns (VenueLoginResponse);
  rpc VenueLogout (VenueLogoutRequest) returns (VenueControlResponse);
  // ListInstruments は Phase 7 のものを拡張 (source="live" を受け付ける)
  rpc SubscribeMarketData (SubscribeRequest) returns (SubscribeResponse);
  rpc UnsubscribeMarketData (UnsubscribeRequest) returns (SubscribeResponse);
  rpc SetExecutionMode (SetExecutionModeRequest) returns (SetExecutionModeResponse);
}

message SetExecutionModeRequest {
  string mode = 1;   // "Replay" / "LiveManual" / "LiveAuto"
}

message SetExecutionModeResponse {
  bool success = 1;
  string error_code = 2;  // "EXECUTION_MODE_PRECONDITION" 等
  string execution_mode = 3;  // 切替後の実際の値
}

message VenueLoginRequest {
  string venue_id = 1;                     // "TACHIBANA" / "KABU"
  string credentials_source = 2;           // "prompt" (default) / "session_cache" / "env"
  string environment = 3;                  // "production" / "demo"
  // 注: password / api_key などの平文資格情報は本 RPC に含めない。
  //     "prompt" 指定時は Python 側が tkinter サブプロセスでログインウィンドウを開く。
  //     第二暗証番号 (Tachibana) は本フェーズで一切扱わない (Phase 9 で発注時に収集)。
}

message VenueLoginResponse {
  bool success = 1;
  string error_code = 2;
  string venue_state = 3;
  int32 instruments_loaded = 4;
}

message SubscribeRequest {
  string instrument_id = 1;
  repeated string channels = 2;            // "price"/"trades"/"depth"
}
```

- `server_grpc.py` に上記ハンドラを実装。各ハンドラは `ModeManager` 経由でガードを通す。
- proto 再生成: `uv run python -m grpc_tools.protoc ...`（既存スクリプト準拠）。

### 3.5 Rust UI: 接続フロー & 表示

- `src/trading.rs`:
  - `VenueState` enum（Python と同期）と `VenueStatusRes` Resource を追加
  - `ExecutionMode` enum (`Replay` / `LiveManual` / `LiveAuto`) と `ExecutionModeRes` Resource を追加。`LiveAuto` は Phase 10 用スタブで Phase 8 では選択不可（`EXECUTION_MODE_PRECONDITION` で reject）
  - `BackendStatusUpdate::VenueChanged { state, venue_id, instruments_loaded }` を追加
  - `BackendStatusUpdate::ExecutionModeChanged { mode }` を追加
  - `GetState` の戻り値から `venue_state` / `execution_mode` を吸い上げて Resource を更新
- `src/ui/menu_bar.rs`:
  - File メニューの下に **Venue メニュー** を追加（枠は Phase 7 で予約済み）
  - `Connect → Tachibana (Demo) / Tachibana (Prod) / kabuStation (Verify) / kabuStation (Prod)` のサブ項目
  - `Disconnect` 項目
  - クリックで `VenueConnectRequested(venue_id, env)` イベント発火 → backend へ `VenueLogin(credentials_source="prompt")` RPC を投げる。
  - **Rust 側にはログインフォームを実装しない**（資格情報を Rust プロセスに乗せないため）。クリック後は Python 側のログインウィンドウがフォーカスを取り、ユーザがそこで入力 → 結果が `VenueStateBadge` に反映されるのを待つだけ。
  - **重要**: Venue → Connect は **mode 切替を伴わない**。Replay 稼働中でも実行可能で、成功すると Sidebar Tickers が Live universe で更新される
- `src/ui/footer.rs`:
  - 既存の `ReplayStateBadge` の左に **`ExecutionModeToggle`** (§3.5.1) を追加
  - 既存の `ReplayStateBadge` の右隣に `VenueStateBadge` を追加（DISCONNECTED=gray / CONNECTED=cyan / SUBSCRIBED=green / ERROR=red）
  - **両バッジ常時表示**: Replay と Venue は独立に遷移するため、どちらの状態も常に見えるようにする
  - **時刻表示は ExecutionMode に従う**（§0.5.1）:
    - `ExecutionMode == Replay`: `ReplayTimeLabel` を主表示 (monospace 16px)。例 `2024-04-15 09:30:00 JST (replay)`
    - `ExecutionMode == Live`: wall clock を主表示。例 `2026-05-14 15:42:31 JST (live)`
    - 副表示として「相手側時刻」を小さく表示する案もあるが、混同を避けるため MVP は **主時刻のみ表示**
- `src/ui/sidebar.rs`:
  - **銘柄リストは Live universe を優先ソースとする**（§0.5.1 例外規定）:
    - Venue ログイン成功時 → `ListInstruments(source="live")` の結果で Tickers Resource を上書き。Replay モードでもこちらが見える
    - Venue 未ログイン時 → `ListInstruments(source="replay")` の結果にフォールバック（Phase 7 と同じ挙動）
    - 数千銘柄になる Live 側のために検索ボックス + 仮想スクロールを必須
    - `venue_hint` でタブ / セクション分けはせず単一リスト（**和集合ではなく上書き**、§0.5.1 にあるとおり最新の取引可能 universe を見せるのが目的）
  - **「最新価格」列の振る舞い** は ExecutionMode に従う（§0.5.1）:
    - `Replay` mode: Replay engine の最新バー close を表示。Live tick が流れていても無視
    - `Live` mode: Live tick の最新 mid を表示
  - 銘柄クリック動作:
    - `Replay` mode: `SelectedSymbol` 更新のみ。Replay engine が該当銘柄のバーを引いていれば Kline に反映。引いていなければ Kline は空（バー履歴の遡及ロードは Phase 8 のスコープ外）
    - `Live` mode: `SelectedSymbol` 更新 + `SubscribeMarketData` 発行

### 3.5.1 ExecutionModeToggle (Footer)

Footer 左端に明示的なモード切替トグルを置く。「いま自分はどちらモードか」「切替操作の入口」を 1 箇所に集約する。

```
[ Replay  |  Manual  |  Auto ]   ReplayState: RUNNING   VenueState: SUBSCRIBED (Tachibana)
```

- **3 値セグメントコントロール**。ラベル: `Replay` / `Manual`（Live Manual）/ `Auto`（Live Auto）
- 現モードがハイライト。`Auto` は Phase 10 まで常に **grayed out**（クリック不可）
- クリックで対象モードへ切替試行 → `SetExecutionMode(target_mode)` RPC を発行
- 前提条件不足時の挙動:
  - `Replay` → `Manual` クリック時に未ログインなら: 確認ダイアログ「Live モードに切り替えるには venue ログインが必要です。今すぐログインしますか？ [ログイン] [キャンセル]」 → 承認で `VenueLogin` を起動、成功後に `SetExecutionMode(LiveManual)` を再試行
  - `Manual` / `Auto` → `Replay` クリック時に戦略未ロードなら: 確認ダイアログ「Replay モードに切り替えるには戦略ファイルを開く必要があります。[開く] [キャンセル]」 → 承認で File → Open Strategy のフローへ
  - `Auto` クリック時は常に「Phase 10 で実装予定」ダイアログを表示（Phase 8 では選択不可）
- **mode 切替は Replay/Venue いずれの稼働も止めない**。「Manual → Replay」切替後も Venue 接続は継続して market data 購読され、Sidebar の Live 銘柄欄も表示され続ける。発注経路の宛先だけが Replay simulator に切り替わる
- 確認ダイアログ（特に Live → Replay）に「**注意**: Live 注文の発射経路が無効になります。既存の Live ポジションは venue 側にそのまま残ります」の警告を出す（Phase 9 で発注経路を実装したら活きる）

### 3.6 UI: モード関連の UX フロー

「Replay と Live は並列稼働可能。発注経路の宛先だけが ExecutionMode で決まる」を踏まえた UX。

- **Replay 中の Venue → Connect**: 確認ダイアログ無しで即座に `VenueLogin` を発火。成功時に Sidebar Tickers の Live セクションが populate される。Replay engine は **止めない**
- **Live 接続中の File → Open Strategy**: 確認ダイアログ無しで戦略ロード可能。`StrategyEditorWindow` が開き、`[▶ Run]` を押せば Replay engine が並列に立ち上がる。Venue 接続は **切らない**
- **ExecutionMode の切替** は §3.5.1 の Footer トグル経由で明示的に行う。Venue → Connect / File → Open は ExecutionMode を勝手に切り替えない
- **Venue → Disconnect 時に ExecutionMode=LiveManual または LiveAuto なら**: 確認ダイアログ「Venue を切断すると Live モードを維持できません。Replay モードに切り替えますか？ [切替えて切断] [キャンセル]」（戦略未ロードなら「IDLE に戻ります」と案内）
- **File → New / Unload 時に ExecutionMode=Replay なら**: 確認ダイアログ「戦略をアンロードすると Replay モードを維持できません。Live (Manual) モードに切り替えますか？ [切替えてアンロード] [キャンセル]」（未ログインなら「IDLE に戻ります」と案内）
- 確認ダイアログは Phase 7 の `ModalLayer` 機構を流用

### 3.7 Live Market Data → 既存パネル + LadderWindow 新設

- **既存パネル (KlineChartWindow / BuyingPowerPanel / PositionsPanel / OrdersPanel)** — Snapshot Reducer は Replay と同じ実装を使うため、これらは **無改修** で Live モードでも動くのが目標。
- **バー集約** — Live は tick / quote を 1m / 5m / 1D に集約する必要があるため、`live/aggregator.py` で BarAggregator を一段挟む（Nautilus 標準 `BarBuilder` を流用）。
- **LadderWindow (新設、Phase 7 から延期分)** — Phase 8 で初めて板情報 (depth) のデータソースが手に入るため、ここで実装する。
  - 実装位置: `src/ui/floating/ladder.rs` (Phase 7 で予約していたファイル名をそのまま使う)
  - MVP: bid/ask × 10 行 + LAST 行 (read-only、クリック発注なし)
  - `e-station` の `src/screen/dashboard/panel/ladder.rs` (1382 行) からの移植
  - データ源:
    - **kabu**: WebSocket PUSH の `Sell1..Sell10` / `Buy1..Buy10` フィールド (kabu skill §「PUSH メッセージ形式」参照)。10 段固定で揃う
    - **Tachibana**: EventWebSocket の板気配。venue/環境によっては 5 段までしか出ないため、不足行はプレースホルダで埋めて 10 行のレイアウトを維持
  - `TradingState` に新フィールド `depth: Option<DepthSnapshot>` を追加（Live venue 由来、Replay engine は埋めない）
  - **可視性は `ExecutionMode` で決まる**（§0.5.1 時間軸ルール）:
    - `ExecutionMode == Replay`: **強制非表示**。Live venue にログイン中で depth が流れていても画面には出さない（replay_time と wall clock の混在禁止）。ユーザの手動 ON は不可
    - `ExecutionMode == Live`: 表示。`VenueState < SUBSCRIBED` のときは「Venue 未購読」プレースホルダ
  - UI_LAYOUT には Live モード時の位置・サイズだけを保存。Replay 切替時は entity を despawn し、Live 復帰時に保存位置で再 spawn

---

## 4. File Layout

```
python/engine/
├── mode_manager.py        [NEW]   # Replay/Live 排他制御
├── live_runner.py         [NEW]   # Live 系統のエントリポイント
├── live/                  [NEW]   # venue 非依存の枠組み
│   ├── __init__.py
│   ├── adapter.py                 # LiveVenueAdapter Protocol / LiveEvent
│   ├── state_machine.py           # VenueStateMachine
│   ├── event_bus.py               # adapter → reducer の asyncio Queue
│   ├── aggregator.py              # tick → bar (Nautilus BarBuilder ラッパ)
│   ├── instrument_mapping.py      # InstrumentId 正規化 (.TSE / .OSE)
│   ├── login_dialog_runner.py     # tkinter サブプロセスエントリ (python -m ...)
│   └── logging.py                 # secrets masking filter (sUrl* / password)
├── exchanges/             [NEW]   # venue 固有プロトコル (Rust に同等実装を作らない)
│   ├── __init__.py
│   ├── tachibana.py               # LiveVenueAdapter 実装
│   ├── tachibana_url.py           # build_request_url / func_replace_urlecnode (R2/R9)
│   ├── tachibana_auth.py          # next_p_no / current_p_sd_date / check_response (R4/R6)
│   ├── tachibana_codec.py         # Shift-JIS / ^A^B^C / "" → [] (R7/R8)
│   ├── tachibana_ws.py            # sUrlEventWebSocket クライアント
│   ├── tachibana_master.py        # CLMEventDownload マスタ
│   ├── tachibana_file_store.py    # tachibana_session.json (R3)
│   ├── tachibana_login_flow.py    # debug env 取込み + tkinter 橋渡し
│   ├── kabusapi.py                # LiveVenueAdapter 実装
│   ├── kabusapi_url.py            # BASE_URL_PROD/VERIFY (1 箇所限定, R1) / symbol_key (R4)
│   ├── kabusapi_auth.py           # POST /token / X-API-KEY / check_response (R3/R7)
│   ├── kabusapi_ratelimit.py      # OrderBucket / WalletBucket / InfoBucket (R5)
│   ├── kabusapi_register.py       # 50 銘柄 RegisterSet (R6)
│   ├── kabusapi_ws.py             # WebSocket (ping_interval=None, R8)
│   └── kabusapi_login_flow.py     # 入力フィールド定義 + 本体プロセス LISTEN ping
├── models.py                      # TradingState に mode / venue_state 追加
├── core.py                        # get_current_state に venue 情報を含める
├── server_grpc.py                 # 5 つの新 RPC ハンドラ
└── proto/engine.proto             # RPC + message 追加

src/
├── trading.rs                     # VenueState / VenueStatusRes / RPC 呼び出し
└── ui/
    ├── menu_bar.rs                # Venue メニュー追加
    ├── footer.rs                  # VenueStateBadge
    └── sidebar.rs                 # mode に応じたティッカー切替

docs/plan/assets/
└── phase8-architecture.drawio.svg [TODO]   # §1.1 図の正本

src/ui/floating/
└── ladder.rs              [NEW]    # Phase 7 から延期分、bid/ask × 10 行 + LAST
```

---

## 5. Implementation Order

各ステップ完了時点で `cargo run` できる状態を維持する。Live API は本番接続せずとも **モックアダプタ**で UI → backend の往復を通せるよう、Step 1 で `MockVenueAdapter` を先に作る。

1. **Step 1 — Skeleton & MockVenueAdapter**:
   - `live_runner.py` / `live/adapter.py` / `live/state_machine.py` のスケルトン
   - `MockVenueAdapter`（固定銘柄 3 つ、ランダムウォーク価格を秒間 1 tick yield）
   - `ModeManager` の排他ロジックと unit test
2. **Step 2 — gRPC RPC & 並列稼働確認**:
   - 新 RPC を proto に追加 (`VenueLogin` / `VenueLogout` / `SubscribeMarketData` / `UnsubscribeMarketData` / `SetExecutionMode`) → stubs 再生成
   - `ListInstruments` に `source="live"` 対応を追加（`source="replay"` は Phase 7 で実装済みの想定）
   - Rust `trading.rs` から RPC を叩き、`MockVenueAdapter` 経由で `VenueState` が `SUBSCRIBED` まで進むことを確認
   - **Replay 実行中でも `VenueLogin` が成功する** ことを確認（旧 `MODE_CONFLICT` 仕様は撤回されているため、reject されないことが正）
   - `SetExecutionMode("Live")` を未ログイン状態で叩くと `EXECUTION_MODE_PRECONDITION` で reject されることを確認
3. **Step 3 — UI 表示 (ExecutionModeToggle + バッジ)**:
   - `VenueStateBadge` を Footer に追加
   - **`ExecutionModeToggle` を Footer 左端に追加**（`Replay ⇄ Live` セグメントコントロール）
   - `Venue → Connect (Mock)` メニュー項目
   - mock で `SUBSCRIBED` になると Footer バッジが緑になり、トグルで `Live` 側に切替可能になる挙動を確認
   - Replay 中（`ReplayState=RUNNING`）でも Venue → Connect が受理されること、両バッジが並列に正しく表示されることを目視確認
4. **Step 4 — Snapshot Reducer 接続 + LadderWindow 新設**:
   - `MockVenueAdapter` の tick を `reducer` 経由で `TradingState` に流す
   - `KlineChartWindow` が Live モードで mock データを描画できることを確認
   - **`src/ui/floating/ladder.rs` を新設** (Phase 7 から延期分)。`TradingState.depth` の bid/ask × 10 行 + LAST 行を描画。`depth == None` 時は「板情報なし (Replay モード)」プレースホルダ
   - `MockVenueAdapter` に固定の 10 段 depth 生成を加えて、Ladder が更新されることを確認
5. **Step 4.5 — Python tkinter ログインサブプロセス**:
   - `live/login_dialog_runner.py` を実装（`python -m engine.live.login_dialog_runner --venue <id> --env demo` で起動可能）
   - venue 固有の入力フィールド定義は `exchanges/tachibana_login_flow.py` / `exchanges/kabusapi_login_flow.py` に置く
   - `credentials_source="prompt"` で Rust から RPC を叩くとサブプロセスが立ち上がり、stdout JSON が `VenueState` を遷移させるまでを mock adapter で確認
   - headless 環境（DISPLAY 無し）で `NO_DISPLAY_AVAILABLE` が返ることを確認
   - `TACHIBANA_ALLOW_PROD` / `KABU_ALLOW_PROD` 未設定時に各 prod 選択肢がグレーアウトされることを確認
6. **Step 5 — kabuステーション実装**:
   - `exchanges/kabusapi*.py` を `.claude/skills/kabusapi/SKILL.md` に従って実装（`kabusapi_url` / `kabusapi_auth` / `kabusapi_ratelimit` / `kabusapi_register` / `kabusapi_ws` / `kabusapi.py` の順）
   - 検証環境（`http://localhost:18081/kabusapi/`）で本体起動 → `VenueLogin` → `/token` 取得 → `ListInstruments` → `PUT /register(3 銘柄)` → WebSocket で板更新が Ladder に反映、までの E2E（Windows 上で手動実行、CI は httpx-mock のみ）
   - `kabusapi_register.RegisterSet` の 50 銘柄上限 / `GET /board` の自動登録も合算する挙動 を unit test
   - `kabusapi_ratelimit` の 5 / 10 / 10 req/s token-bucket を unit test
   - `kabusapi_ws` で `ping_interval=None` + `asyncio.wait_for(ws.recv(), 3600)` のハング検出パスを unit test
   - `KABU_ALLOW_PROD` 未設定で本番 18080 への接続が Python URL builder で拒否されることを unit test
6. **Step 6 — Tachibana 実装**:
   - `exchanges/tachibana*.py` を `.claude/skills/tachibana/SKILL.md` に従って実装
   - 検証環境（`demo-kabuka.e-shiten.jp`）で `VenueLogin` → `ListInstruments` → 板情報購読 → Ladder 反映までの E2E
   - Shift-JIS / `p_errno` / 仮想 URL / `^A^B^C` 区切りの取り扱いを単体テスト
   - 第二暗証番号は **Phase 8 では収集しない** (Phase 9 で発注時に iced modal、skill F-H5)
7. **Step 7 — Sidebar 銘柄検索**:
   - `ListInstruments` 結果を仮想スクロールで表示
   - インクリメンタル検索（コード前方一致 / 名称部分一致）
8. **Step 8 — Auto-Reconnect & Error Surfacing**:
   - 指数バックオフ再接続
   - `VenueState == ERROR` 時に Footer 右下にトースト表示
9. **Step 9 — Polish**:
   - Instruments parquet キャッシュの日次更新
   - secrets masking ログフィルタの統合テスト
   - drawio アーキ図 `phase8-architecture.drawio.svg` を作成

---

## 6. Success Criteria

- Replay 実行中に Venue メニューから接続すると、Replay engine は **中断されずに継続し**、ログイン成功時点で Sidebar Tickers が Live universe で上書きされる（銘柄一覧は時間軸ルールの例外、§0.5.1）。
- Replay モード中は LadderWindow が **強制非表示** で、Footer の時刻表示は `replay_time` のみ、Sidebar の「最新価格」列は Replay engine 由来の値のみが表示される（時間軸混在チェック、ExecutionMode トグル切替後も維持される）。
- Live モードに切替えると LadderWindow が表示され、Footer 時刻が wall clock に切替わり、Sidebar の「最新価格」が Live tick で更新される。同一画面に Replay 時刻と Live 時刻が同時に並ぶことが無い（grep 検証および目視確認）。
- Footer の `ExecutionModeToggle` で `Replay ⇄ Live` を切替できる。前提条件不足時は確認ダイアログ経由で前段操作（Login / Open Strategy）へ誘導される。
- Venue メニュー → `kabuStation (Verify)` 接続（kabuステーション本体 18081 起動済み前提）→ `/token` 取得 → 銘柄登録 3 件 → Sidebar に表示、までが手動 E2E で通る（**Windows 上で実施**、CI では同等を httpx-mock で再現）。
- Sidebar から 1 銘柄選択 → 数秒以内に Kline / Ladder が Live データで更新を開始する（kabu の場合は `PUT /register` → WebSocket PUSH 経由）。
- kabuステーション本体が未起動 / API オプション無効 / 本体ログアウト状態の各ケースで原因別エラー (`KABU_STATION_NOT_RUNNING` / `KABU_API_DISABLED` / `4001001`) が分離されてトーストに出る。
- 同様の手動 E2E が `Tachibana (Demo)` でも通る。
- 同時購読が 50 銘柄を超えるリクエスト（REST + PUSH の合算、`GET /board` 自動登録も含む）は `kabusapi_register.RegisterSet` で事前検出され、`SUBSCRIPTION_LIMIT_EXCEEDED` で reject され UI に明示される。サーバ側の `4002001` への依存ではなく事前抑制で達成すること。
- kabu の発注系 5 req/s / 余力系 10 req/s / 情報系 10 req/s 流量制限が `kabusapi_ratelimit` の token-bucket で事前抑制され、`4002006` を踏まない（unit test + 連打 E2E で確認）。
- 認証エラー時、`VenueState=ERROR` バッジが赤で表示され、エラーコード（`AUTH_FAILED` / `KABU_STATION_NOT_RUNNING` / `KABU_TOKEN_EXPIRED` / `KABU_API_DISABLED` / `NETWORK_ERROR` 等）がトーストに出る。kabu トークン失効 (`4001005`) は 1 回 retry 後にダイアログ再表示される。
- `KABU_ALLOW_PROD` / `TACHIBANA_ALLOW_PROD` 未設定での本番接続試行が **Python URL builder で拒否**される（Rust 側で防がない、unit test で確認）。
- ログを全文 grep してもユーザ名・パスワード・API key・Tachibana の仮想 URL (`sUrlRequest` / `sUrlMaster` / `sUrlPrice` / `sUrlEvent` / `sUrlEventWebSocket`) が平文で出現しない（secrets masking テスト、Tachibana skill R10）。
- Tachibana の 2 回目以降の起動が `tachibana_session.json` のみで成立する（env 未設定でも JST 当日付なら復元できる、Tachibana skill S3）。
- `TACHIBANA_ALLOW_PROD` 未設定での本番接続試行が Python URL builder で拒否される（unit test）。
- Replay と Live で **同じ Snapshot Reducer / 同じ UI コード** が使われており、`src/ui/floating/kline.rs` / `ladder.rs` には Phase 8 起因の差分が無い（あっても mode 表示の 1 行のみ）。
- Rust 側に `exchange/src/adapter/tachibana.rs` / `src/connector/auth.rs` の立花拡張 / 立花用ログイン UI が存在しない（grep で確認、Tachibana skill 「Rust 側に置かないもの」）。

---

## 7. Open Questions & ADRs

### ADR: ExecutionMode は 3 値（Replay / LiveManual / LiveAuto）
当初の `Replay | Live` の 2 値設計を撤廃し、3 値に変更する。理由:
1. **発注操作の有無で Live の意味が全く異なる** — 手動発注（LiveManual）ではユーザが判断してボタンを押す。自動発注（LiveAuto）では戦略コードが注文を出す。UI の責務・安全確認の重みが根本的に違うため、同一 mode に束ねるべきでない。
2. **UI_LAYOUT の保存先が異なる** — LiveManual は戦略 `.py` を持たないため standalone JSON に保存。LiveAuto は戦略 `.py` が設定ファイルを兼ねるため Replay と同じ sentinel block に保存。2 値では保存先を判定できない。
3. **Phase 10 との整合** — Replay-to-Live 昇格（Phase 10）は `Replay → LiveAuto` のプロモーションとして自然に表現できる。`Replay → Live` という曖昧な昇格より意図が明確。

Phase 8 では `LiveAuto` は grayed-out スタブとして定義のみ行い（Phase 10 で実装）、実質的に `Replay ⇄ LiveManual` の 2 値切替として動作する。

### ADR: UI_LAYOUT は `.py` 同名の `.json` サイドカーに保存する（sentinel block 方式不採用）
Phase 7 設計段階では「`.py` 末尾に sentinel block を埋め込む」案があったが、**不採用**とし、`.py` と同名の `.json` ファイル（サイドカー）に保存する方式を採用する。

理由:
1. **`.py` を汚さない** — UI レイアウトは Python 実行と無関係。コードレビュー / git diff にノイズが入る。sentinel block を後から追加するパースロジックも不要になる。
2. **`.py` + `.json` を 1 組の入力ファイルとして扱える** — `test_strategy_daily.py` と `test_strategy_daily.json` がセットであることがファイル名で自明。IDE / OS のファイラでも一緒に並ぶ。
3. **JSON は既存ツールで読み書きできる** — `serde_json` で直接 serialize/deserialize。JSON5 パーサや Python literal 正規化ロジックが不要。
4. **LiveManual 例外の扱いが自然に決まる** — Replay / LiveAuto は対応する `.py` があるのでサイドカーが成立。LiveManual は `.py` が存在しないため、`cache_dir` の standalone `live_manual_layout.json` で別処理する。この場合でも保存形式（JSON）は統一。
5. **Promote to Live (Phase 10) との親和性** — `replay_strategy.py` + `replay_strategy.json` のペアをそのまま LiveAuto に昇格できる。sentinel block を除去する変換ステップが不要。

### ADR: Live は別 runner として完全分離する
`replay_runner.py` に live モードのフラグを足す案を採らず、`live_runner.py` を独立させる。理由: (1) Replay は決定論的なシミュレータでデバッグの中心。Live コードが混ざると再現性が壊れる。(2) `TradingNode` 由来の live execution を将来取り込むときも、Replay 系統に影響を与えないため。(3) Replay engine と Live venue を並列稼働させても、互いのコードパスが独立しているため副作用が漏れない。

### ADR: 認証と Execution は排他しない（旧 ModeManager 排他案の撤回）
初稿では `ReplayState >= LOADED` と `VenueState >= CONNECTED` を相互排他にし、片方が稼働中はもう片方を `MODE_CONFLICT_*` で reject する設計だった。これを **撤回** し、両 state machine を完全独立に動かす。代わりに `ExecutionMode: Replay | Live` という新しい明示的フラグを導入し、**発注経路の宛先だけ**を排他する。

理由:
1. **ユーザ要望**: 「Phase 7 のローカル固定銘柄一覧をログインしたら更新する」という UX を成立させるには、Replay 稼働中の venue ログインを許可する必要がある。排他制約を残したままだとログイン操作のたびに Replay セッションを破棄する確認ダイアログが出てしまい、流れが断たれる。
2. **データ読取と発注は別問題**: market data 購読は read-only なので Replay と並行しても整合性に影響しない。誤発注事故は「発注 RPC が Replay simulator と Live venue のどちらに飛ぶか」だけが問題で、それは ExecutionMode で十分に守れる。
3. **将来の Promote to Live (Phase 10) との親和性**: Phase 10 は「Replay で動いた戦略をそのまま Live に昇格」する。これは本質的に Replay と Live が同時に立ち上がっている瞬間を必要とする（昇格のためのウォームアップ）。排他制約があると Phase 10 でその制約を解く再設計が必要になる。今から撤廃しておくほうが整合的。
4. **ExecutionMode の前提条件チェックで十分**: `Live` への切替は `VenueState >= CONNECTED` 必須、`Replay` への切替は `ReplayState >= LOADED` 必須、というガードを ModeManager に残せば「未認証で Live 注文」「戦略未ロードで Replay 注文」は構造的に発生しない。

### ADR: 時系列データはモードの時間軸と一致するもののみ表示する (混在禁止)
Replay モードは `replay_time`（過去のシミュレーション時刻）、Live モードは wall clock（現在の実時刻）で進行する。両者の時系列データ（price / depth / trades / fills）を同じ画面に並べると **「この値はいつのもの？」というユーザ誤解** を生み、戦略判断を誤らせる致命的なバグ源になる。

そのため Phase 8 では「Live venue にログインして depth が流れている＝Replay モードでも Ladder を見せる」案を **採用せず**、ExecutionMode に厳密に従わせる。具体的には:
- LadderWindow は `ExecutionMode == Replay` のとき強制非表示（手動 ON 不可）
- KlineChartWindow は ExecutionMode 側のバーソースのみを描画（Replay 時に Live aggregated バーを混ぜない）
- Sidebar の「最新価格」列も ExecutionMode 側の値だけを出す
- Footer 時刻は ExecutionMode の主時間軸のみ表示

これにより「ある時点で画面に出ている全ての時系列数値は同じ time domain」という不変条件が常に成立する。

### ADR: 銘柄一覧は時間軸ルールの例外として Live venue から取得する
§0.5.1 の時間軸ルールの **唯一の例外** が銘柄一覧。理由:
1. **銘柄一覧は静的メタデータで時系列ではない** — 銘柄コード・名称・呼値単位・売買単位は「いつの時刻のものか」という問いが本質的に意味を持たない
2. **J-Quants catalog は historical 断面で最新情報を持たない** — Replay 用の catalog は過去日付のスナップショットなので、最新の上場/廃止情報を欠く。Replay モードのままでも「いま取引可能な銘柄は何か」を知りたいユーザ要望は妥当
3. **Replay と Live でセッションが分断されない UX** — Replay で見つけた銘柄を Live で発注へ進める導線が、Sidebar の銘柄リストを一貫させることで自然に成立する

そのため Venue ログイン成功時、ExecutionMode に関わらず Sidebar Tickers は Live universe（Tachibana / kabu のマスタ）で上書きされる。未ログイン時のみ Replay catalog (`source="replay"`) にフォールバック。なお、「最新価格」列は時系列なので例外の対象外で、ExecutionMode に従う。

### ADR: ExecutionMode は明示的な UI トグルで切替える
ExecutionMode 切替は Venue → Connect や File → Open Strategy などの **副作用として暗黙に**起こさず、Footer の `ExecutionModeToggle` を経由する明示操作のみとする。理由: (1) ログインしただけで Live モードに切り替わると「ちょっと銘柄一覧を見たかっただけ」のユーザを驚かせる。(2) 発注経路の宛先という重要事項を暗黙切替にすると事故の温床になる。(3) UI 上「いま自分はどちらモードか」を Footer の 1 箇所で常に確認できる利点もある。

### ADR: 資格情報を Rust UI 側に持たせない
平文の API key / パスワードを gRPC ペイロードに乗せないため、`VenueLoginRequest` には `credentials_source` だけを乗せる。Backend が prompt / session_cache / env から自前で resolve する。理由: gRPC ログ・コアダンプ・OS の swap 経由で漏れる経路を構造的に塞ぐ。Rust 側に資格情報 UI を作る必要も無くなる。

### ADR: keyring / 平文 file credentials を採用しない
Phase 8 初稿では `credentials_source` に `"keyring"` / `"file"` も含めたが、Tachibana skill が **`tachibana_session.json` ファイルキャッシュ一本**でセッション永続化する規約 (R3 / S3) を確立しているため、keyring も平文資格情報ファイルも採用しない。理由: (1) 永続化される資料は「ユーザー名/パスワード」ではなく「短命の仮想 URL」だけにし、漏洩時の被害範囲を 1 営業日に限定する。(2) Python 側に 2 種類の credential store 抽象（keyring vs file）を保つコストを払わない。kabu 側は token 再取得が軽量なため永続化自体を諦め、`exchanges/kabu*.py` のメモリ保持のみで足りる。

### ADR: 立花プロトコル固有コードは Python `exchanges/` にだけ置く
Tachibana skill が `python/engine/exchanges/tachibana*.py` 集約を規定しているため、これに完全準拠する。`exchange/src/adapter/tachibana.rs` / `src/connector/auth.rs` の立花拡張 / `src/screen/login.rs` の立花フォーム / Rust 側の立花 WebSocket クライアントは Phase 8 のスコープから明示的に除外する。理由: (1) URL ビルド・Shift-JIS・p_no 採番・`^A^B^C` パース等が Rust と Python に二重実装されると齟齬が必ず発生する。(2) 仮想 URL の取り扱いは「セッション秘密」のためマスク・寿命管理を 1 箇所に閉じたい。(3) skill の規範に逆らうとレビューが通らない。

### ADR: kabuStation プロトコル固有コードも Python `exchanges/kabusapi*.py` にだけ置く
kabusapi skill が **`python/engine/exchanges/kabusapi*.py` 集約**を規定しているため、立花と同じ方針を kabu にも適用する。Rust 側に新設するのは下記のみ:

- `engine-client/src/dto.rs` — `Venue::KabuStation` バリアント追加（既存 enum に追加するだけ）
- `exchange/src/adapter.rs` — `Venue::KabuStation` / `Exchange::KabuStation*` 列挙子

Rust 側に **置かない**もの: `exchange/src/adapter/kabu.rs` venue adapter / 立花とは別系統の kabu WebSocket クライアント / `/token` 認証コード / X-API-KEY ヘッダ組立て / `RegisterSet` の重複実装 / 流量制限の重複実装 / `Symbol@Exchange` 文字列の組立て。これらはすべて `kabusapi*.py` 側に閉じる。理由: 立花 ADR と同じ（二重実装の齟齬回避、トークン寿命管理の一元化、skill 規範遵守）。

### ADR: kabu のトークンはファイル永続化しない（session_cache 経路は持たない）
kabu の `/token` 発行トークンは本体終了/ログアウト/別トークン発行で失効する短命なものなので、Tachibana の `tachibana_session.json` のような session cache は **意図的に作らない**（kabu skill S4）。理由: (1) 永続化しても次回起動時に失効している確率が高く、復元判定コードを保守する価値が低い。(2) `/token` 再発行は数十 ms で完了するため、毎起動取り直しで体感速度に影響なし。(3) ファイルキャッシュを増やすと「漏洩経路を増やす」「stale token で謎エラーを踏む」の両方のリスクが上がる。`credentials_source` の `"session_cache"` は kabu では **`UNSUPPORTED_FOR_VENUE`** で reject する。

### ADR: kabu WebSocket は `ping_interval=None` を必須化する
kabusapi の WebSocket サーバは RFC 6455 準拠の PONG を返さない（PING payload と不一致の空 PONG）。`websockets` ライブラリの既定 `ping_interval=20s` のままだと **30 秒ごとに timeout 切断ループ**が発生する（kabu skill R8 / Issue #40）。Phase 8 では `kabusapi_ws.connect()` が **`ping_interval=None` を強制**し、代わりに `asyncio.wait_for(ws.recv(), timeout=3600)` で無メッセージハングを検出して再接続する。Tachibana の手動 pong 必須仕様とは正反対の挙動なので、`live/adapter.py` から共通 keepalive ヘルパーを引き出すのは避け、各 venue で個別に持つ。

### ADR: ログインウィンドウは Python 側で出す（プロジェクト唯一の UI 例外）
本プロジェクトは原則「UI は Bevy (Rust) に一本化」だが、**ログインフォームに限り Python のサブプロセスから tkinter で表示する**例外を設ける。

理由:
1. **資格情報を Rust → Python に受け渡したくない**。Rust 側で入力させると `VenueLoginRequest` に password を載せるか、別 RPC で平文を送る必要があり、gRPC 上の暗号化が無い localhost 通信ではコアダンプ / プロセスダンプ / メモリスキャンで漏れる経路が増える。Python プロセス内で完結させれば、資格情報はそのまま venue adapter のメモリにしか乗らない。
2. **headless 運用でもユーザー対話によるログインが必要**。Rust UI を起動しない CI 検証や、別マシンの backend にリモートで `python -m engine` だけ走らせる構成でも、その場でログインダイアログを出したい。Rust UI に依存させると headless ではログインできなくなる。
3. tkinter は Python 標準ライブラリで追加依存ゼロ。Bevy より遥かに小さなフォームウィンドウで十分なため、専用 UI フレームワークは不要。

結果として Rust UI は「Venue メニューでログイン開始トリガを発火 → Python 側のダイアログ完了を待つ → `VenueStateBadge` に結果が反映される」という一方向の責務だけを持つ。ログインフォーム描画責務は Rust から完全に切り離される。

実装上は **サブプロセス**として `python -m engine.live.login_dialog_runner` を起動する（`Tk.mainloop()` が server_grpc の asyncio loop / Bevy をブロックしないため）。venue 固有の入力フィールド定義は `exchanges/{tachibana,kabu}_login_flow.py` に置き、`login_dialog_runner` 側は venue 共通の枠だけを描画する。

### ADR: 第二暗証番号 (Tachibana) は Phase 8 で扱わない
Tachibana の発注時必須項目である第二暗証番号 (`sSecondPassword`) は **Phase 8 のログインダイアログに含めない**。Tachibana skill F-H5 に従い、Phase 9 の発注 UI 内で **iced modal** (Rust 側) で取得し、Python の venue adapter メモリにのみ保持・idle forget タイマーで自動消去する。理由: (1) Phase 8 は read-only 市場接続であり、第二暗証番号を必要としない。(2) ログイン時に集めて長時間保持すると漏洩窓が広がる。発注のたびに再入力させて寿命を短く保つ。(3) env / セッションキャッシュに書かない原則を Phase 8 で乱さない。

### ADR: debug ビルドのみ env 自動ログインを許可する
`DEV_TACHIBANA_USER_ID` / `DEV_TACHIBANA_PASSWORD` / `DEV_TACHIBANA_DEMO` / `KABU_API_PASSWORD` の自動ログインは **debug ビルドの Python のみ**が読む。release では env を完全に無視し、`credentials_source="env"` を `ENV_DISABLED_IN_RELEASE` で reject する。理由: (1) 配布バイナリにユーザー資格情報の env 取込みパスを残さない。(2) 本番ユーザーが誤って `.env` を作ってリポジトリへ commit する事故経路を塞ぐ。(3) Tachibana skill S1 と一致させる。

### ADR: 本番接続は `TACHIBANA_ALLOW_PROD=1` で明示解禁する
demo 既定 (`DEV_TACHIBANA_DEMO` 未設定 = demo) に加え、本番 URL `https://kabuka.e-shiten.jp/e_api_v4r8/` への接続は **Python URL builder が `TACHIBANA_ALLOW_PROD=1` env を検出したときに限り**許可する。理由: (1) Tachibana skill R1 が「本番接続で実弾が飛ぶ」「URL リテラルは `tachibana_url.py` の 1 箇所限定 (F-L1)」を明文化している。(2) demo/prod 切替を UI チェックボックス一つで誤爆させない。(3) prod 解禁は env で意図表明する形にし、CI 自動運転からは外す。

### ADR: 発注経路は Phase 8 で握らない
`nautilus_trader` の `TradingNode` を Live でホストすると ExecEngine が venue に注文を発射できてしまう。Phase 8 では `DataEngine` のみホストし、`ExecEngine` は **インスタンス化しない**。これにより「読み取り専用」を型レベルで担保する。Phase 9 で発注経路を追加する際は、別途明示的なフラグと確認 UX を入れる。

### ADR: kabu ステーション本体プロセスへの依存を許容する
kabu adapter は `localhost:18080` への HTTP 接続前提。アプリ側でプロセス起動まで自動化はしない（ユーザに手動起動を求める）。理由: kabu ステーション GUI のライセンス・自動操作の規約上、起動自動化はリスクが高い。代わりに「起動していない」ことを `KABU_STATION_NOT_RUNNING` で即時検出して UX 上明示する。

### ADR: Replay と Live Auto のデータソース非対称性（Phase 10 への前提制約）

Replay モードと Live Auto モードはデータソースが根本的に異なるため、以下の**構造的非対称性**がある。Phase 10 (Replay-to-Live 昇格) の実現にはこの非対称性を埋める層が必要。

| | Replay モード | Live Auto モード |
|---|---|---|
| **価格データ形式** | J-Quants OHLCV バー（日足 / 分足） | Live tick + board depth |
| **分足バー** | J-Quants から既製品として取得 | **存在しない**。tick から自前で集約 |
| **板情報 (depth)** | **存在しない** | EventWebSocket / PUSH で取得 |
| **時間軸** | `replay_time`（過去） | wall clock（現在） |

この非対称性が意味すること:
- **分足戦略を Live Auto で動かすには bar aggregation が必須** — `live/aggregator.py` の `BarBuilder` が tick → 分足バーを生成し、Replay 側と同じ `KlineUpdate` イベント形式で戦略に渡す必要がある（§3.3 の `live_runner.py` が担う）。
- **板情報（depth）を Replay 戦略で参照しているコードは Live Auto でのみ有効** — `SCENARIO.granularity=Minute` で板情報を参照する戦略は、Replay 環境では板データが流れてこないため動作が変わる。この問題の責任は「戦略が環境依存になっている」ことにある（Phase 10 の Strategy Portability 設計で対処）。
- **Phase 8 での実装含意**: `live/aggregator.py` は tick → 分足バーの集約精度が Phase 10 の実用性を決める。Nautilus の `BarBuilder` を使い、bar close / partial bar push の両方に対応する実装にしておく。

→ この制約は Phase 10 の設計ドキュメントと `Tranceparent Headless Replay.md` に転記する。

### ADR: Tachibana の `sJsonOfmt=4` 固定
Tachibana API の応答フォーマットは複数あるが、JSON5 互換でパーサが楽な `sJsonOfmt=4`（フィールド名 + JSON）に固定する。他フォーマットを許容するとアダプタが膨らむため。Skill `.claude/skills/tachibana/` の規約に従う。

### Open Question: Tick → Bar 集約の正本はどこに置くか
`live/aggregator.py` 内で Nautilus 標準の `BarBuilder` を直接呼ぶか、`engine_runner` 側に集約レイヤを置いて Live/Replay 両対応にするか未定。Step 4 着手時に決める。前者の方が Phase 8 内で完結するため初期実装としては前者を採用予定。

### Open Question: 複数 Venue の同時接続を将来許可するか
Phase 8 では 1 Venue のみ。`VenueStateMachine` を venue ごとに持つよう作っておけば将来拡張可能だが、`ModeManager` の排他ルールが複雑化する。Phase 10 以降で必要性が出てから再評価。

---

## 8. Verification & Decision Log

（実装着手後に追記する。Phase 7 と同じく日付ヘッダで commit / 検証結果 / 残課題を記録する。）
