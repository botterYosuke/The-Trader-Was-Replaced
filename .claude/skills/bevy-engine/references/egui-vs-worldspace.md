# egui vs world-space sprite ウィンドウ — どちらを使うか

このプロジェクトは 2 つの UI 流派を併用している。新しいパネルを足すときは
**どちらかを選ぶ**。混ぜると保守不能になる。迷ったら下表で判定する。

## 早見表

| 観点 | bevy_egui (immediate) | world-space sprite (retained) |
|---|---|---|
| 既存例 | menu_bar, strategy_editor, footer の一部 | buying_power, positions, orders, run_result, chart |
| カメラのパン/ズームに追随 | **しない**（画面固定） | **する**（world 座標） |
| テキスト編集（IME, copy/paste） | 強い（標準でサポート） | bevy_cosmic_edit が必要 |
| 状態管理 | フレーム毎に再構築。Resource ベース | Entity + Component（ECS native） |
| Z オーダー | 自動（後勝ち、`order` で調整） | 自前（`WindowManager::max_z`） |
| ドラッグ移動 | egui が標準で対応 | `observe(Pointer<Drag>)` を自前 |
| 描画コスト | フレーム毎にレイアウト計算 | spawn 後は Transform 更新だけ |
| 見た目のカスタマイズ | egui スタイル制約あり | Sprite で自由（rim light など） |
| 表示/非表示 | system 内 `if` で `.show` をスキップ | entity を despawn / spawn |

## 判定フローチャート

```
そのパネルの目的は？
├─ テキスト編集が中心 / フォームっぽい / ダイアログ
│   → egui（strategy_editor を踏襲）
├─ チャート上のオーバーレイ・常駐情報表示・ドラッグして並べたい
│   → world-space sprite（buying_power 等を踏襲）
└─ 画面端に固定（ステータスバー等）
    → world-space sprite + 画面端追従 system（footer / menu_bar 流）
```

## 各流派のテンプレ起点

**egui**: `src/ui/strategy_editor.rs` をコピーして始める。
- system 関数だけ書けばいい。spawn 不要。
- 表示条件は system の先頭で `if buffer.original_path.is_none() { return; }` のように。

**world-space sprite**: `src/ui/buying_power.rs` をコピーして始める。
- `spawn_<panel>_panel(commands)` で `spawn_floating_window` を呼ぶ
- update system で marker component を Query して中身を書き換える
- `PanelKind` を追加、dispatcher の match に arm 追加、`UiPlugin` に system 登録

## 既存パネルの流派

- world-space: buying_power, positions, orders, run_result, chart
- egui: strategy_editor
- world-space (画面追従): footer, menu_bar, sidebar
