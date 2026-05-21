# EVENT / WebSocket ストリームのパース規約

## 区切り文字

受信データは ASCII 制御文字を区切りとして項目を羅列する:

| 記号 | コード | 意味 |
| :--- | :--- | :--- |
| `^A` | `\x01` | 項目区切り |
| `^B` | `\x02` | 項目名と値の区切り |
| `^C` | `\x03` | 値と値の区切り（複数値時） |
| `LF` | `\x0A` | メッセージ区切り（WebSocket は `^A` 末尾でも区切る） |

形式例: `項目A1^B値B1^A項目A2^B値B21^CB22^CB23^A...`

## キー命名

キーは `<型>_<行番号>_<情報コード>` 形式:

- 例 `p_1_DPP` → 型 `p`（プレーン文字列）・行番号 `1`・情報コード `DPP`（現在値）
- 行番号は `p_gyou_no`（1〜120）と対応
- パース: Python ヘルパー `tachibana_codec.parse_event_frame(data: str) -> list[tuple[str, str]]`

## URL パラメータ（重要な固定値）

EVENT I/F は **REQUEST と違い通常の `key=value&...` 形式**で組み立てる（R2 例外）。サンプルの並び順と値に合わせる:

```
{sUrlEvent}?p_evt_cmd=ST,KP,EC,SS,US,FD
           &p_eno=0            ※イベント通知番号（0=全件、再送時は指定値の次から）
           &p_rid=22           ※株価ボード・アプリ識別値（No.2: e支店・API、時価配信あり）
           &p_board_no=1000    ※固定値（株価ボード機能）
           &p_gyou_no=N[,N,...]    ※行番号（1-120）
           &p_issue_code=NNNN[,NNNN,...]   ※銘柄コード
           &p_mkt_code=NN[,NN,...]         ※市場コード
```

`p_evt_cmd` の種別（マニュアル別紙「EVENT I/F 利用方法」 p3/26 および [`e_api_event_receive_tel.py` l.534-544](../samples/e_api_event_receive_tel.py/e_api_event_receive_tel.py)）:

| コード | 意味 | 通知契機 |
| :--- | :--- | :--- |
| `ST` | エラーステータス | 発生時 |
| `KP` | キープアライブ | 5 秒間通知未送信時 |
| `FD` | 時価情報 | 初回はメモリ内スナップショット（全データ）、以降は変化分のみ |
| `EC` | 注文約定通知 | 初回は当日分の未削除通知を接続毎に再送、以降は発生時 |
| `NS` | ニュース通知 | 初回再送、以降発生時。**重いため必要時のみ** |
| `SS` | システムステータス | 初回再送、以降発生時 |
| `US` | 運用ステータス | 初回再送、以降発生時 |
| `RR` | 画面リフレッシュ | 現時点不使用（指定しても無視） |

## EC（注文約定通知）の情報コード — e-station 参照実装で確定

- `api_event_if.xlsx`（EVENT I/F データ仕様）は本リポジトリ未同梱だが、**EC 通知の情報コードは
  参照実装 `C:\Users\sasai\Documents\e-station` の `python/engine/exchanges/tachibana_event.py`
  + `architecture.md §6` で確定している**（2026-05-21 確認）。EC を扱うときはまず e-station の
  当該ファイルを ground truth として参照すること。
- 確定済み EC 情報コード（`型_情報コード` 形式・行番号なしのフラットキー）:

  | キー | 意味 | 備考 |
  | :--- | :--- | :--- |
  | `p_NO` | 注文番号（venue_order_id = `sOrderNumber` 相当） | |
  | `p_EDA` | 約定枝番（trade_id。立花内部 `p_eda_no`） | **重複検知キー**（`(p_NO, p_EDA)`、再接続の全件再送を弾く） |
  | `p_NT` | 通知種別 | `1`=受付 / `2`=約定 / `3`=取消 / `4`=失効 |
  | `p_DH` | 約定単価 | 取消/失効時は欠落 |
  | `p_DSU` | 約定数量（この約定分） | 取消/失効時は欠落 |
  | `p_ZSU` | 残数量 | `0`=全約定（→ FILLED）、`>0`（→ PARTIALLY_FILLED） |
  | `p_OD` | 約定日時（JST `YYYYMMDDHHMMSS`） | UTC ms へ変換 |

- **EC は side / issue_code / 原数量を持たない**。累計約定数量は `発注数量 - p_ZSU`、銘柄・売買は
  注文セッション（発注時レジストリ）から join する。
- **残る Demo 検証事項（2 点）**: ① 口座レベル EC を購読する EVENT URL の構成（e-station は EC を
  per-ticker FD 接続に相乗りさせる）。② `build_event_url` の **comma を `%2C` にエンコードする問題** —
  e-station `build_ws_url` は「サーバが `%2C` を認識しない」として `p_evt_cmd` に **raw comma** を送る。
  FD 購読 URL にも影響しうるため Demo で要確認。

## SS（システムステータス）による閉局 / ログアウト検知 — Phase 9 Step 7

- SS は `CLMSystemStatus` マスタレコードを EVENT WS で配信する。閉局 / 本体ログアウトの検知に使う
  （`tachibana.py::_handle_system_status`）。フィールド（`mfds_json_api_ref_text.html` で確定）:

  | キー | 意味 | 値域 |
  | :--- | :--- | :--- |
  | `sCLMID` | 機能 ID | `CLMSystemStatus` |
  | `sSystemStatus` | システム状態 | `0`=閉局 / `1`=開局 / `2`=一時停止 |
  | `sLoginKyokaKubun` | ログイン許可区分 | `0`=不許可 / `1`=許可 / `2`=不許可(時間外) / `9`=管理者のみ |

- 判定（**実装の確定挙動**）: **真の閉局（`sSystemStatus == "0"`）** または **真のログイン不許可
  （`sLoginKyokaKubun == "0"`）** のみを「本体ログアウト → 要再ログイン」とみなす。
  `sSystemStatus == "2"`（一時停止）と `sLoginKyokaKubun == "2"`（不許可・時間外）/ `"9"`（管理者のみ）は
  **平常の非アクション（= open 相当）** として扱い logout を撃たない（一時停止は session loss ではなく
  transient halt、時間外は平常の閉場であり、いずれも偽の再ログイン modal を避ける意図的判断）。
  SS は**接続毎に初回再送**されるため、open→closed の **遷移時のみ 1 回**通知する
  （debounce。`_last_system_open` で管理、login/logout でリセット）。
  回帰テスト: `test_ss_suspended_status2_does_not_fire` / `test_ss_login_kubun_out_of_hours_does_not_fire`。
- ⚠️ **要 Demo 検証（§5.1 layer-3）**: 上の表は REQUEST/Master API の `CLMSystemStatus` 仕様で、
  **EVENT フレームでのフィールド名 prefix（`s*` か `p_*` 変種か）は実 Demo 未確認**（EC 購読 URL /
  comma エンコードと同じ Demo-pending 事項）。判別フィールド欠落時は安全側（通知しない）に倒すこと。
- 🔎 **Demo での確定方法**: `_handle_system_status` は既知フィールド（`sSystemStatus` /
  `sLoginKyokaKubun`）が無い SS フレームを受けると、実フィールドキーをセッション 1 回だけ
  `WARNING`（`"tachibana SS frame lacks ... actual field keys=[...]"`）でログする。Demo 接続後の
  初回 SS（接続毎に再送）でこの行を grep すれば、推測せずに本物の prefix を確定できる。確定後は
  `_handle_system_status` のキー名を実値に合わせ、`sLoginKyokaKubun=="2"`（時間外）を logout
  扱いにすると平常の時間外で偽 modal が出る点も併せて見直すこと。

## 注意点

- **EVENT URL に `\n` や `\t` を入れない**（制御文字でサーバがエラー応答する）。
- WebSocket 接続は Python 側 `python/engine/exchanges/tachibana_ws.py` に集約する。`websockets.connect(uri, ping_interval=None, ping_timeout=None)` で `websockets` ライブラリの自動 ping を無効化し、**受信ループで ping を受け取ったら手動で pong を返す**（[`e_api_websocket_receive_tel.py:710-723`](../samples/e_api_websocket_receive_tel.py/e_api_websocket_receive_tel.py#L710) の `pong_handler` を参照）。
- `p_errno:"2"` は仮想 URL 無効 → 再ログイン（電話認証から）。
- EVENT 受信データはメッセージ単位で `LF` または `^A` 終端。一塊のチャンクに複数メッセージが含まれるため、受信バッファを蓄積しながら区切り子で分割する必要がある。
- 受信本文も Shift-JIS。REQUEST と同じく UTF-8 前提で読むと銘柄名・ニュース本文が文字化けする。
