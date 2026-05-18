# kabuStation 実機 smoke test 手動手順 (Phase 8 §3.2 D1)

`test_kabusapi_smoke.py::test_kabu_real_login_smoke` は `@pytest.mark.slow` +
`@pytest.mark.kabu_live` で隔離されており、CI では走らない。Windows 開発機で
手動実行する。

## 前提

1. kabuStation 本体 (Windows GUI) が起動 + ログイン済みで、設定 → API → 「APIを
   利用する」が ON。検証環境ポートは **18081**。
2. API パスワードを `.env` に置く:
   ```
   DEV_KABU_API_PASSWORD=<api_password>
   ```
3. `KABU_ALLOW_PROD` は **設定しない** (verify 既定で十分。本番 18080 は別途検証時のみ)。

## 実行

```powershell
uv run pytest python/tests/exchanges/test_kabusapi_smoke.py -m "slow and kabu_live" -v
```

接続不可 (`ConnectError` / `OSError`) は `pytest.skip` に変換するため、本体未起動
でも red にはならない (= 「skip = setup 漏れ」と読む)。

## 期待結果

```
test_kabu_real_login_smoke PASSED
```

token 取得が確認できれば smoke は通過。subscribe / events の実機検証は Phase 9 の
別 smoke で取り扱う。

## tachibana smoke との対比

| venue | 実機 smoke の自動化可否 | 備考 |
|---|---|---|
| tachibana (demo) | `@pytest.mark.slow` で `-m slow` から自動実行可 | `DEV_TACHIBANA_USER_ID` / `DEV_TACHIBANA_PASSWORD` のみで走る (リモート demo) |
| kabu (verify) | 本ドキュメント参照 (手動) | Windows GUI 本体が必要 |
