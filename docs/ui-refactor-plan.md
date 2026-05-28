# UI Refactor Plan — order_panel split (deferred to #46 Slice B)

本ドキュメントは `src/ui/order_panel.rs`（1,219 行）の分割計画のみを記述します。**#48 では実装しません**。実装は **#46 Slice B**（component helper 課題）で行います。

## 1. 現状

- `src/ui/order_panel.rs` 1 ファイルに以下が同居:
  - 発注フォーム（symbol / side / qty / price フィールド・validation・OrderForm Resource）
  - 確認モーダル（ConfirmModal 開閉・Escape 優先順位・PlaceOrder 送出）
- 1,219 行は本 repo の UI ファイルとして 3 番目に大きく、form / modal の責任が混在しています。

## 2. 分割案

```
src/ui/order_panel/
├── mod.rs              ← Plugin entry / module re-export
├── form.rs             ← 発注フォーム（OrderForm Resource / OrderForm UI / validation）
└── confirm_modal.rs    ← 確認モーダル（ConfirmModal Resource / Modal UI / PlaceOrder 送出）
```

## 3. 分割境界

- **`form.rs` が持つもの**: `OrderForm` Resource、フォーム UI entity の spawn/update system、field commit、validation、`OrderSubmitRequested` Event(Message) の発火。
- **`confirm_modal.rs` が持つもの**: `ConfirmModal` Resource、モーダル UI entity の spawn/despawn、Escape/outside-close ハンドリング、最終 `TransportCommand::PlaceOrder` 送出。
- **`mod.rs` が持つもの**: `OrderPanelPlugin` の Bevy `Plugin` impl と `add_systems` 配線、子モジュール re-export のみ。

## 4. Resource / Event(Message) の所有権

| 型 | 所有 module | 読む側 |
|---|---|---|
| `OrderForm` Resource | `form.rs` | `confirm_modal.rs`（form 内容を表示するため read のみ） |
| `ConfirmModal` Resource | `confirm_modal.rs` | `form.rs` は触らない |
| `OrderSubmitRequested` Message | `form.rs` で発火 | `confirm_modal.rs` で receive → モーダル open |
| `TransportCommand::PlaceOrder` | `confirm_modal.rs` で send | — |

注意: Bevy 0.18 では `Event` が `Message` にリネームされています。

## 5. モーダル open trigger 経路図

```
[OrderForm UI]
   │ user clicks Submit button
   ▼
[form.rs::handle_submit_click_system]
   │ writes Message<OrderSubmitRequested>
   ▼
[confirm_modal.rs::open_on_submit_system]
   │ reads Message<OrderSubmitRequested>
   │ spawns ConfirmModal entity
   ▼
[confirm_modal.rs::handle_confirm_button_system]
   │ reads OrderForm Resource (snapshot)
   │ sends TransportCommand::PlaceOrder
   │ despawns ConfirmModal
```

## 6. 実装担当 issue

- **本 issue (#48)**: この plan doc のみ。実装しない。
- **#46 Slice B**: 実コード分割 + `ModalSkeleton` component helper への乗せ替え。
