# Phase 8 §3.2 実 venue I/O 実装 — 引継ぎ

リポジトリ: `C:\Users\sasai\Documents\The-Trader-Was-Replaced`
ブランチ: `impl/8-venue-login-skeleton`
HEAD: `7be3282 test(tachibana): cover TickerEventWsHub fanout / aclose / on_connect / exception isolation (Phase 8 §3.2 A3.2b)`

## 完了済み

| Step | Commit | 内容 | テスト |
|---|---|---|---|
| A0 ✅ | (前 session) | httpx / pytest-httpx / pytest-asyncio (auto) / pytest-timeout / freezegun 導入 | 522 維持 |
| A1.1 ✅ | (前 session) | `tachibana_url.py` 全面置換: newtype 5 種 + build_auth_url + guards | +16 |
| A1.2 ✅ | (前 session) | `tachibana_auth.py` 末尾追記: PNoCounter / TachibanaSession / StartupLatch | +13 |
| A1.3a ✅ | `9d80903` | 例外型を e-station 互換に拡張、login / validate_session_on_startup を stub | 29 PASS |
| A1.3b ✅ | `9d80903` | `test_tachibana_auth.py` に login() RED 13 件写経 | 29 PASS + 13 FAILED + 13 ERROR (RED 観測) |
| **A1.4 ✅** | `474a873` | `login()` GREEN 実装 (e-station 写経 + 本 repo 例外規約に適応) | exchanges 132 passed |
| **A1.5 ✅** | `55702d7` | `TachibanaAdapter.login()` wire-up (env 経路のみ、session_cache/prompt は NotImplementedError) | exchanges 139 passed |
| **C1 ✅** | `476a7f2` | `live_adapter_factory` + `serve()` 配線 (`live_venue` kwarg → factory 注入) | regression 627 passed (preexisting 3 件は MISSING_STRATEGY_FILE、C1 起因 0) |
| **A2 ✅** | `2cefbac` + `f64e57f` | `tachibana.fetch_instruments` 実装 (CLMEventDownload マスタ DL + parser 純関数化) | exchanges GREEN / regression 594 passed |
| **A3.1 ✅** | `1c8d82e` + `2e551f5` | `tachibana_ws.py` 新設: `is_market_open` (JST 前場/後場+クロージング) + `FdFrameProcessor` (F3 side rule / F4 first-frame & DV-reset / F17 ts_ms / 10-level depth) | exchanges GREEN (+11 in test_tachibana_ws) |
| **A3.2a ✅** | `a3aaa00` + `88a1820` | `TachibanaEventWs` async client: websockets.connect + per-frame `asyncio.wait_for` dead-frame timeout + `FdFrameProcessor` delegate (+ `websockets` dep) | exchanges GREEN (test_tachibana_ws 18 passed) |
| **A3.2b (部分) ✅** | `55a63b4` + `a84b5cc` | `TickerEventWsHub` 写経 (subscribe/unsubscribe lifecycle, fanout, aclose, on_connect/on_close, dispatch 例外吸収) + 最小 lifecycle test 1 件 | test_tachibana_ws 19 passed |
| **A3.2b (完了) ✅** | `7be3282` | `TickerEventWsHub` 未テスト挙動 5 件 append (duplicate subscribe / fanout / aclose+on_close / on_connect 再発火 / dispatch 例外隔離) | test_tachibana_ws 24 passed, tachibana scope 156 passed |

> note: 同 session で想定外 commit (`60b7bc0` / `eb13ed2` = `.claude/skills/zed/src/` 1.5M 行) を rebase --onto で drop。`c5215df ｓ` は `16d7099 zed` として再積み (149 行 SKILL.md add)。保険 branch `backup/pre-rebase-1519f69` 残置 (要らなければ `git branch -D`)。

## 残タスク

6 件:

- **A3.3** `TachibanaAdapter.subscribe` / `unsubscribe` / `events` 配線
- **A3.3** `TachibanaAdapter.subscribe` / `unsubscribe` / `events` 配線
- **B1** `kabusapi.login` (env + `/token`)
- **B2** `kabusapi.fetch_instruments` (lazy = 空 list、subscribe 時 GET `/symbol`)
- **B3** `kabusapi_register.RegisterSet` (50-symbol LRU)
- **B4** `kabusapi_ws.py` + adapter (ping_interval=None 必須)
- **D1** `-m slow` smoke test (tachibana + kabu 各 1 本)
- **D2** 計画書更新 + 完了報告

## 必読 (次 session)

- 本ファイル
- `docs/plan/Phase 8 - Live Venue and Market Data.md` (進捗スナップショット §3.2 / §11 Tips)
- A1.4/A1.5 で生まれたコード: `python/engine/exchanges/tachibana_auth.py` (login 本体) と `python/engine/exchanges/tachibana.py` (adapter wire)
- 参考実装 (別 repo、同作者):
  - `C:\Users\sasai\Documents\e-station\python\engine\exchanges\tachibana_master.py` (A2 写経元)
  - `同 tachibana_ws.py` (A3 写経元)
  - `同 kabusapi_*.py` (B1-B4 写経元)
  - `同 tachibana_login_flow.py` (将来 A1.5+ で session_cache / prompt を実装する際の写経元)

## スキル

`/pair-relay` `/tachibana` `/kabusapi` `/tdd-workflow` `/nautilus_trader`

## ⚠️ 既知の罠 (今までの learnings)

- **pair-relay 1 往復 ≈ 25k tokens** — Driver/Navigator に full diff を運ぶため。**1 session で 2-3 subtask が限界**
- **Navigator の事前 test count 見積もりは当てにならない** — 削除/分割が絡むと外す。Driver は collect-only で実数確認。passed 全数 ≤ collected で failed/errors=0 ならカウント差は気にせず GREEN 判定可
- **pytest-httpx teardown が RED を二重カウント** — `13 FAILED + 13 ERROR` のような対称な数字は同一テストの teardown ノイズ。GREEN 化で両方消える
- **pair-relay Navigator の import パス推測罠** — `from python.engine.*` を書くと Driver が ModuleNotFoundError。新規 test を指示する前に既存 test の冒頭 import を 1 つ Read で確認
- **subagent はネスト spawn 不可** — 司令塔 → Navigator → Driver の 2 層が上限
- **MEMORY** — `~/.claude/projects/.../memory/MEMORY.md` の関連エントリも合わせて参照

## A1.5 で持ち越したスコープ

- `credentials_source == "session_cache"`: NotImplementedError stub のまま (test 1 本でガード済み)
- `credentials_source == "prompt"`: NotImplementedError stub のまま (test 1 本でガード済み)
- `DEV_TACHIBANA_DEMO` env: 不採用 (constructor `environment` を単一の真実とした)。将来 session_cache 実装時に再評価
- `validate_session_on_startup`: stub のまま (StartupLatch 経由のみ、test 未追加)
- 第二暗証番号: 完全に env 外 (発注 step で iced modal + 即時 forget)

## 次 session 開始指示テンプレ

```
plans/phase8-a1-handoff.md を読み、A2 (tachibana.fetch_instruments、CLMEventDownload マスタ DL) から再開。
ユーザー決定 L76 「順次 A→B、tachibana 完了後 kabu」に従い、tachibana 側 A2 → A3 を先に通してから
B1-B4 (kabu) へ移る。D1 smoke は A/B 完了後。写経元は e-station tachibana_master.py (L39 参照)。
pair-relay 1 往復で 1 subtask、1 session 2-3 subtask が現実的ライン。
```

## ユーザー決定事項 (確定、変更しない)

1. **両 venue 並列**: 順次 (A→B、tachibana 完了後 kabu)
2. **MVP スコープ**: prompt UI defer、kabu fetch_instruments は空 list、tachibana マスタは最低限 mapping
3. **smoke test**: `-m slow` で実接続 smoke 1 本ずつ
4. **serve() 配線**: Phase 8 全体の範疇で完了
5. **HTTP 実装**: e-station フル移植 (Option B、minimal wrap でない)
6. **URL builder**: tachibana_url 内 inline、prod guard は後送り、`BASE_URL_*` は AuthUrl 型

## Known preexisting failures (not C1-induced)

C1 完了時点の regression で観測された 3 件の FAILED は、いずれも C1 配線とは無関係の preexisting failure:

- 失敗パターン: `MISSING_STRATEGY_FILE` (strategy file fixture が見つからない系)
- C1 起因 0 件 (live_adapter_factory / serve() 配線まわりの test は全 passed)
- regression 集計: **627 passed**、preexisting 3 FAILED は別タスクで扱う
- 後続タスク (A2/A3/B*/D1) は本 3 件を baseline として進めて良い

## レビュー指摘対応

### High-2 → Low 格下げ (本 session スコープ外)

**指摘**: fetch_instruments の master DL 経路で `p_errno` / `sResultCode` を
check_response に通していない。session expired が空リスト `[]` として握り
つぶされる可能性がある (tachibana.py:120-127)。

**判断**: 本 session では着手せず、Low に格下げ。

**根拠**:
- HTTP レベルのエラーは `resp.raise_for_status()` (tachibana.py:121) で raise
  されるため、session expired が 401/403 で返るなら問題は発生しない。
- Tachibana の CLMEventDownload エラー応答が単一 dict 形式 (`{"p_errno": ...}`)
  で 200 OK と共に返るかは未確認。e-station 公式サンプルにも check_response
  相当は無く、エビデンスが弱い。
- 架空のエラー応答 fixture でテストを書いても実形式と乖離するリスクがあり、
  false sense of security になる。

**フォローアップ条件** (いずれかで再起動):
1. 実 Tachibana で session expired を踏んで `fetch_instruments()` が `[]` を
   返す再現が取れたとき。
2. Tachibana API ドキュメントで master DL のエラー応答形式が確定したとき。
3. A2 以降で order 系 (CLMKabuNewOrder 等) を実装する際、同じ
   `check_response` パターンが必要になり master 側も合わせて入れる流れに
   なったとき。

**現状の防御**:
- `resp.raise_for_status()` で HTTP 系エラーは raise
- `build_instruments_from_master_records` は不明 sCLMID record を silent drop
  するので、`p_errno` 含む dict が混ざっても InstrumentRaw 構築には影響しない
  (ただし空 list 化のリスクは残る)

### Low: chunked-response 統合テスト改善 (本 session 据え置き)

**背景**: Step 3 で `fetch_instruments` を `client.stream()` + `aiter_bytes()`
経路にリファクタしたため、test 側で複数 chunk を yield する httpx mock を
組めば、本物の chunked decode の統合テストにできる。

**現状**: 既存 test は単一 chunk 相当で通っており、chunked decode の
boundary 跨ぎ branch (decode_clm_yobine_record の record 境界またぎ) は
unit test (`test_decode_clm_yobine_record_*`) で別途カバー済み。
integration 観点での coverage 漏れは無い。

**着手条件**: 実 Tachibana で chunked boundary 起因の decode bug が観測
されたとき、または adapter refactor で stream 経路の挙動を変えるとき。
