# マスタダウンロード（CLMEventDownload）の特殊ルール

`CLMEventDownload` は他の REQUEST と流れが違う:

- ストリーム形式（Python の `urllib3` で `preload_content=False` 相当、または httpx の `stream` API）で全量配信。
- 1 レコードの終端は `}`、**全体の終端はレコード `{"sCLMID":"CLMEventDownloadComplete", ...}` の到着**。Python サンプルは `str_terminate = 'CLMEventDownloadComplete'` を定数化している。
- 接続先は `sUrlMaster`（`sUrlRequest` ではない — [`e_api_get_master_tel.py:578-580`](../samples/e_api_get_master_tel.py/e_api_get_master_tel.py#L578)）。
- `sJsonOfmt` は `"4"` を使う（1 行 1 JSON 形式、ファイル保存・後続パース向け。`"5"` を使うと区切れなくなる）。
- 受信チャンクをバイト列で蓄積し `byte_data[-1:] == b'}'` で 1 レコード分として Shift-JIS デコード → `json.loads`（[`e_api_get_master_tel.py:492-518`](../samples/e_api_get_master_tel.py/e_api_get_master_tel.py#L492)）。
- データ量が大きいため、メモリ展開ではなくストリーム処理を守ること。
- **遅延が大きい（落とし穴 / Issue #32）**: 全量 master DL は数十秒〜分オーダーになりうる。adapter は
  `_MASTER_READ_TIMEOUT = 600.0` を予算化している。これを呼ぶ同期経路
  （`server_grpc` の `ListInstruments(live)` store-miss fallback → `LiveRunner.fetch_instruments_blocking`）に
  **共有の短い timeout（`_live_timeout_s = 5s`、login/account/subscribe 用）を流用してはいけない**。
  5s だと `concurrent.futures.TimeoutError`（`str()` が空）になり UI に空の `Error: fetch_instruments failed:`
  を出す。専用の長い timeout（`_instruments_timeout_s`）を使うこと。
- **二重 DL に注意**: `InstrumentsScheduler` の login 直後 refresh と、UI の `[+ Add]`（picker）が同時に
  `fetch_instruments()` を呼ぶと master DL が 2 本走りうる。adapter 側で singleflight（in-flight task を
  `asyncio.shield` で共有 + done-callback で `_instruments_inflight` を消す）を持たせて 1 本に集約する。

## マスタデータ識別子（`sTargetCLMID`）

- `CLMIssueMstKabu` 銘柄マスタ（株）
- `CLMIssueSizyouMstKabu` 銘柄市場マスタ（株）
- `CLMIssueMstSak` 銘柄マスタ（先物）
- `CLMIssueMstOp` 銘柄マスタ（OP）
- `CLMIssueMstOther` 日経平均・為替など
- `CLMOrderErrReason` 取引所エラー理由コード
- `CLMDateZyouhou` 日付情報
