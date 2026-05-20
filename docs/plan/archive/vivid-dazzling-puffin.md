# Plan: Venue メニュー項目をバックエンド設定 venue に応じて非表示化

## Context

バックエンドは `--live-venue TACHIBANA | KABU`（または未指定）で起動し、接続後は片方の venue しか受け付けない（`VENUE_MISMATCH` エラー）。しかし Rust フロントエンドのメニューには TACHIBANA/KABU 両方の Connect 項目が常時表示されており、無効な側を押しても VenueLogin が弾かれるだけになっている。接続確立後にバックエンドの設定 venue を取得し、無効な側のメニューを非表示にする。

---

## アーキテクチャ概要

既存の `GetState` JSON ポーリング（~1 秒間隔）に `configured_venue` フィールドを追加し、現行の venue_state / execution_mode の diff 検出パターンを踏襲する。新規 RPC や proto 変更は不要。

```
backend --live-venue TACHIBANA
  → GetState JSON に configured_venue="TACHIBANA" を追加
  → Rust polling loop が検出 → BackendStatusUpdate::ConfiguredVenueDiscovered 送信
  → apply_status_update で VenueStatusRes.configured_venue を更新
  → hide_unconfigured_venue_items_system が kabu 2 項目を Display::None に
```

---

## 変更ファイル一覧

| ファイル | 変更内容 |
|---|---|
| `python/engine/models.py` | `TradingState` に `configured_venue: Optional[str]` を追加 |
| `python/engine/server_grpc.py` | `GetState()` の `model_copy(update={...})` に `configured_venue=self._live_venue_id` を追加 |
| `src/trading.rs` | `BackendTradingState`・`BackendStatusUpdate`・`VenueStatusRes` を拡張。**既存テストの `BackendTradingState` リテラル 3 箇所に `configured_venue: None,` を追記**（下記 Step 2 注記） |
| `src/main.rs` | polling ループに `prev_configured_venue` diff 検出と `ConfiguredVenueDiscovered` 送信を追加。`apply_status_update` にアームを追加 |
| `src/ui/menu_bar.rs` | `hide_unconfigured_venue_items_system` を追加。**既存テストの `VenueStatusRes` リテラル 2 箇所に `..Default::default()` を追記**し、テスト用 helper を新システム向けに用意（下記 Step 4 / テスト節） |
| `src/ui/mod.rs` | 新システムを `Update` スケジュールに登録 |

---

## 詳細実装手順

### Step 1 — Python: `configured_venue` フィールド追加

**`python/engine/models.py`**
```python
configured_venue: Optional[str] = Field(
    None,
    description="バックエンド起動時の --live-venue 設定 venue (例: TACHIBANA / KABU)。未設定なら None"
)
```
`venue_id` フィールドの直後に追記。

**`python/engine/server_grpc.py`** (`GetState` メソッド ~line 381)

既存:
```python
state = state.model_copy(
    update={"live_last_error": live_last_error, "last_prices": last_prices}
)
```
変更後:
```python
state = state.model_copy(
    update={
        "live_last_error": live_last_error,
        "last_prices": last_prices,
        "configured_venue": self._live_venue_id,
    }
)
```

### Step 2 — Rust `trading.rs`: 3 箇所を拡張

**`BackendTradingState` serde struct** (`~line 52`)
```rust
#[serde(default)]
pub configured_venue: Option<String>,
```

> ⚠️ **コンパイルブロッカー**: `trading.rs` の既存テストは `BackendTradingState { ... }` を
> 全フィールド明示のリテラルで構築している（`..Default::default()` 不使用）。フィールド追加で
> 以下 3 箇所がコンパイルエラーになるため、各々に `configured_venue: None,` を追記する：
> - `trading.rs:742` `test_backend_update_logic`
> - `trading.rs:802` `test_backend_update_fallback_history_points`
> - `trading.rs:848` `test_backend_update_defensive_cap`

**`BackendStatusUpdate` enum** (既存の `VenueChanged` / `ExecutionModeChanged` の近く)
```rust
ConfiguredVenueDiscovered {
    venue_id: Option<String>,
},
```

**`VenueStatusRes` resource** (既存フィールドの末尾)
```rust
pub configured_venue: Option<String>,
```
`#[derive(Default)]` により新フィールドは `None` で初期化される。

> ⚠️ **コンパイルブロッカー**: ただし「追記のみで OK」なのは `..Default::default()` /
> `VenueStatusRes::default()` を使う構築箇所（`main.rs`、`instrument_picker.rs` のテスト）だけ。
> `menu_bar.rs` のテスト helper は struct リテラルで全フィールドを明示しているため、フィールド
> 追加でコンパイルエラーになる。以下 2 箇所に `..Default::default()` を追記する（または
> `configured_venue: None,` を明示）：
> - `menu_bar.rs:1195` `build_app_for_menu_gating`
> - `menu_bar.rs:1281` `build_app_for_menu_press`

### Step 3 — Rust `main.rs`: polling ループと apply_status_update

**polling ループ** (既存の `prev_venue` / `prev_mode` 宣言の直後 ~line 400)
```rust
let mut prev_configured_venue: Option<Option<String>> = None;
```

**GetState 成功ブロック** (既存の `prev_mode` diff 検出の直後 ~line 853)
```rust
if prev_configured_venue.as_ref() != Some(&state.configured_venue) {
    let _ = status_tx.send(BackendStatusUpdate::ConfiguredVenueDiscovered {
        venue_id: state.configured_venue.clone(),
    });
    prev_configured_venue = Some(state.configured_venue.clone());
}
```
> `prev_configured_venue` の型を `Option<Option<String>>` にすることで、
> 初回（prev=None）と "configured_venue=None が返ってきた" を区別できる。
> 接続が切れて再接続した場合は diff が再発火するので、異なる backend に再接続しても反映される。

**`apply_status_update`** (既存の `VenueChanged` アームの近く ~line 288)
```rust
BackendStatusUpdate::ConfiguredVenueDiscovered { venue_id } => {
    venue_status.configured_venue = venue_id;
}
```

### Step 4 — Rust `menu_bar.rs`: 非表示システム追加

`gate_venue_menu_items_system` の直後に追加:
```rust
pub fn hide_unconfigured_venue_items_system(
    status: Res<VenueStatusRes>,
    mut btn_q: Query<(&MenuItem, &mut Node), With<Button>>,
) {
    if !status.is_changed() {
        return;
    }
    let configured = status.configured_venue.as_deref().map(|s| s.to_ascii_uppercase());
    for (item, mut node) in &mut btn_q {
        let is_tachibana = venue_connect_is_tachibana(item);
        let is_kabu = venue_connect_is_kabu(item);
        if !is_tachibana && !is_kabu {
            continue;
        }
        node.display = match &configured {
            Some(v) if v == "TACHIBANA" && is_kabu => Display::None,
            Some(v) if v == "KABU" && is_tachibana => Display::None,
            _ => Display::Flex,
        };
    }
}
```

> - `configured = None`（未接続 or Replay-only backend）→ すべて `Display::Flex`（現行のまま）
> - `configured = Some("TACHIBANA")` → kabu の 2 項目を `Display::None`
> - `configured = Some("KABU")` → tachibana の 2 項目を `Display::None`
> - 接続が切れて `configured` が None に戻った場合は `Display::Flex` に復元

### Step 5 — `src/ui/mod.rs`: システム登録

`mod.rs` line 227 の `gate_venue_menu_items_system` の直後:
```rust
hide_unconfigured_venue_items_system,
```
import 追加:
```rust
use crate::ui::menu_bar::{
    ...,
    hide_unconfigured_venue_items_system,
};
```

---

## テスト

### Python
- `python/tests/test_models.py`: `TradingState(configured_venue="TACHIBANA", ...)` が parse/serialize できることを確認
- `python/tests/test_grpc_phase8.py`: `GetState` レスポンス JSON に `configured_venue` が含まれることを確認（`self._live_venue_id` が反映されているかのユニットテスト）

### Rust (`src/ui/menu_bar.rs` の `#[cfg(test)]`)

既存の `build_app_for_menu_gating` は (1) `configured_venue` を渡す経路がなく、(2) `gate_venue_menu_items_system`
を登録しているため、そのままでは流用できない。新システム用に専用 helper を追加する（既存 helper は
`..Default::default()` 修正のみで温存）:

```rust
fn build_app_for_hide(configured: Option<&str>) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.insert_resource(VenueStatusRes {
        configured_venue: configured.map(|s| s.to_string()),
        ..Default::default()
    });
    app.add_systems(Update, hide_unconfigured_venue_items_system);

    // tachibana / kabu の Connect ボタンを Node 付きで spawn（display 既定 = Flex）
    let btn_t = app.world_mut().spawn((
        Button, Node::default(), MenuItem::VenueConnectTachibanaDemo,
    )).id();
    let btn_k = app.world_mut().spawn((
        Button, Node::default(), MenuItem::VenueConnectKabuVerify,
    )).id();

    app.update();
    (app, btn_t, btn_k)
}
```

> `hide_unconfigured_venue_items_system` は `is_changed()` ゲートを持つが、Resource を
> `insert_resource` した初回フレームは `is_changed()==true` なので 1 回の `app.update()` で発火する。

この helper で `VenueStatusRes.configured_venue` をセットし、`hide_unconfigured_venue_items_system` を実行:

1. `configured_venue=None` → tachibana/kabu 両方の Node.display == `Display::Flex`
2. `configured_venue=Some("TACHIBANA")` → tachibana は `Flex`、kabu は `None`
3. `configured_venue=Some("KABU")` → kabu は `Flex`、tachibana は `None`
4. **復元テスト**: `configured_venue=Some("TACHIBANA")` で 1 frame 走らせ kabu が `None` を確認後、
   `world.resource_mut::<VenueStatusRes>().configured_venue = None` に変更（`resource_mut` で
   `is_changed()` が再 true）→ もう 1 frame → kabu が `Flex` に復元することを確認。
   ※ case 4 は同一 App 内で 2 frame 回す必要があるため `build_app_for_hide` をそのまま使わず、
   helper が返す `app` を使い回して `resource_mut` → `app.update()` する。

### E2E 手順
1. `python -m engine --token dev --live-venue TACHIBANA` でバックエンド起動
2. `cargo run` でフロントエンド起動
3. Venue メニューを開く → "Connect kabuStation (Verify/Prod)" が非表示になっていることを確認
4. `--live-venue KABU` で再起動 → tachibana 項目が非表示になることを確認
5. `--live-venue` なしで再起動 → 全 Connect 項目が表示されることを確認

---

## 留意点

- proto 変更なし。`configured_venue` は `GetState` の `json_data` フィールド内の JSON として流れる
- `prev_configured_venue` の型を `Option<Option<String>>` にするのは意図的：`None`（未検出）と `Some(None)`（backend が `configured_venue=null` を返した）を区別するため
- `is_changed()` ガードにより毎フレームの不要な更新を抑制（既存の `gate_venue_menu_items_system` は毎フレーム走るが、こちらは節約できる）
- **struct リテラル破壊トラップ**: `BackendTradingState` / `VenueStatusRes` への serde/Resource フィールド追加は、`..Default::default()` を使わず全フィールドを明示しているテストフィクスチャを軒並みコンパイルエラーにする。本計画では `trading.rs` の 3 箇所と `menu_bar.rs` の 2 箇所が該当（`instrument_picker.rs` は `..Default::default()` 使用のため影響なし）。フィールド追加時は同型の struct リテラルを全 grep して洗い出すこと
- `hide_unconfigured_venue_items_system` と既存 `gate_venue_menu_items_system` は触る Node フィールドが異なる（`display` vs `BackgroundColor`/`TextColor`）ため競合しない。非表示（`Display::None`）ボタンも gate 側は処理するが無害
