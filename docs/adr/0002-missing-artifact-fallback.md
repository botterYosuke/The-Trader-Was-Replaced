# Listed-Symbols Artifact が無いときは Catalog から生成し、範囲外日付は失敗させない

Replay で `[+ Add]` ピッカーを開くと、scenario.end の日付に対する **Universe**（その日の Listed Symbols）が必要になる。これを毎回 **Catalog**（Nautilus parquet, source of truth）から走査すると ~600 ファイル / cold で ~40s かかるため、結果を **Listed-Symbols Artifact**（end_date 単位の JSON キャッシュ）に永続化し、以後は artifact hit で即返す。**artifact が無いときは失敗にせず Catalog を走査して artifact を生成する**（`ListAllListedSymbols` RPC, `server_grpc.py`）。

## Considered Options

- **範囲外日付（Catalog の最古日より前）**: エラーにせず `success=True, ids=[]` を返し、**空 artifact を負キャッシュ**する。理由: 「データが無い日」は正常な状態であり、UI 側は空 Universe を専用文言 "No instruments for this date"（検索一致なしの "No matches" とは別）で表現できる。エラーにすると毎回 ~40s の再走査を誘発し、ピッカーが永久 spinner に見える。
- **未来日（最新日より後）**: reject せず `resolved_end_date` を Catalog 最新日に **clamp** して返す。フロントは要求時の end_date をキーに保持する（clamp 後の日付ではない — `main.rs` の transport task）。
- **Catalog 自体が無い + artifact も無い**: ここで初めて `success=False, "No catalog_path available"`。フロントは `AvailableInstrumentsFetchFailed` → `last_error` で "Error: ..." を表示。

## Consequences

- artifact は **キャッシュであって source of truth ではない**。Catalog を再生成したら古い artifact は stale になりうる（schema_version / end_date 不一致・破損時は読み捨てて再生成する）。
- 空 artifact の負キャッシュにより、範囲外日付を再度開いても走査は走らない（意図的）。Catalog にデータを足した後に範囲が変わった場合、該当 end_date の artifact を消さないと古い空結果が残る。
- 検証: backend 分岐は `python/tests/test_grpc_list_all_listed_symbols.py`（Case 3=miss→scan→write, Case 6=before_oldest→空, Case 7=future→clamp, Case 8=no catalog→fail）。ただし多くが `@pytest.mark.slow` で**通常スイート（`-m "not slow"`）では走らない**。フロント側の seam 反応は E2E flow C3/C4/J12。
