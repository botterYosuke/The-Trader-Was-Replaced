# UI Theme & Design Tokens (#48)

本ドキュメントは `src/ui/theme/` 配下で定義されるデザイントークン群（Theme / ColorScale / DynamicSpacing / Typography / Elevation）の運用ガイドです。新しい UI コードを書くときに参照してください。

## 1. 概要

- `Theme` Bevy `Resource` が dark variant の全トークンを保持します（`src/ui/mod.rs` で `init_resource::<Theme>()`）。
- system からは `Res<Theme>` で読み、`theme.colors.*` / `theme.status.*` / `theme.typography.*` / `theme.layout.*` 経由でアクセスしてください。
- 直接 `Color::srgb(...)` / `padding: 8.0` / `Transform z = 100.0` を書かない、が大原則です（§8 アンチパターン参照）。

## 2. `ThemeColors` 索引

`src/ui/theme/mod.rs` の `///` doc コメントが一次資料です。よく使うものだけ抜粋:

| フィールド | 用途 |
|---|---|
| `background` | app 全体の root 背景 |
| `surface_background` | パネル / footer / sidebar / menu の表面 |
| `elevated_surface_background` | ポップオーバー / ドロップダウン / ツールチップ |
| `panel_background` | パネル内コンテンツ領域（footer など） |
| `border` / `border_variant` / `border_focused` | 境界線（通常 / 微弱 / focus 中） |
| `text` | 本文・既定の text color |
| `text_muted` | 副次的なラベル（footer の status text 等） |
| `text_placeholder` | input の placeholder |
| `text_disabled` | 操作不能状態のラベル |
| `text_accent` | accent 色の text（links 等） |
| `element_background` | button / chip / 操作要素の通常背景 |
| `element_hover` / `element_active` / `element_selected` | 操作要素の状態色 |
| `accent` / `accent_hover` | primary action（footer ▶ 等） |
| `icon` / `icon_muted` / `icon_disabled` / `icon_accent` | 4 段階の icon color |

裁定: 4 段階あった旧グレー文字色は `text_muted` / `text_disabled` の 2 段階に集約しました。

## 3. `StatusColors` / `PlayerColors` / `SyntaxColors`

`StatusColors` は info / warning / error / success の 3 トリプル（base / background / border）＋取引特有の long/short/bid/ask の 4 トリプル。Run Result / footer status / toast / orders の色は全てここから取ります。

`PlayerColors` は 8 種のチャート系統色（multi-instrument 描画用）。

`SyntaxColors` は #48 では**フィールド宣言のみ**。syntect/tree-sitter との相互変換は #50 で `bevscode` に乗せて実装します。

## 4. `DynamicSpacing`

`src/ui/theme/spacing.rs` の `DynamicSpacing` enum と `.px(density)` API:

| Variant | Compact / Default / Comfortable |
|---|---|
| `Base00` | 0 / 0 / 0 |
| `Base02` | 2 / 2 / 3 |
| `Base04` | 3 / 4 / 5 |
| `Base06` | 4 / 6 / 8 |
| `Base08` | 6 / 8 / 10 |

使い分け: **gap / 細かい inset には `Base04`〜`Base06`、container padding には `Base08`** が指針。旧 footer の `padding: 10.0` は Comfortable density のとき `Base08` (=10) に吸収されます。

## 5. `Typography`

`src/ui/theme/typography.rs`。`HeadlineSize {XSmall, Small, Medium, Large, XLarge}` と `LabelSize {XSmall, Small, Default, Large}` の 2 軸。

- **heading** … パネルタイトル / モーダル見出し。`theme.typography.headline(HeadlineSize::Small).size` のように引きます。
- **label** … footer / button / 小さなメタ情報。footer の text_font はすべて `LabelSize::XSmall` / `Small` / `Default` に置換済みです。
- **body** … 本文・長文。
- **mono** … 等幅。**#48 では宣言のみ**。editor / gutter / 板への配線は #50（`bevscode` 置換）で行います。

## 6. `ElevationIndex`

`src/ui/theme/elevation.rs`。`Transform.translation.z` の直書きを根絶するための tier:

| Variant | z | 用途 |
|---|---|---|
| `Background` | 0 | root 背景 |
| `Surface` | 10 | footer / sidebar / menu / 通常パネル |
| `ElevatedSurface` | 100 | popover / dropdown / tooltip |
| `ModalSurface` | 300 | モーダルダイアログ |
| `Notification` | 500 | toast / safety rail violation |
| `DragOverlay` | 700 | drag preview |

裁定: modal は `ModalSurface`、toast は `Notification`、popover は `ElevatedSurface` を使ってください。

## 7. `Radius` / `Layout` / `Appearance`

- `Radius { sm, md, lg, full }` — `border_radius` 用。
- `Layout { toolbar_h, footer_h, sidebar_w, inspector_w, density }` — UI レイアウトの基本寸法。**注意**: 現時点で footer.rs の生コードは `Val::Px(28.0)` を持ち、`theme.layout.footer_h` のデフォルトは `24.0` です。値の食い違いは #46 Slice B で統一する余地が残っています。
- `Appearance` — Light/Dark 等のテーマ識別（将来 JSON load 用）。

## 8. アンチパターン集

| やってはいけない | 代わりに |
|---|---|
| `Color::srgb(0.07, 0.07, 0.08)` 直書き | `theme.colors.surface_background` 等の意味付きトークン |
| `const BTN_HOVER: Color = ...` のファイルローカル定義 | `theme.colors.element_hover` |
| `padding: UiRect::all(Val::Px(8.0))` | `UiRect::all(Val::Px(DynamicSpacing::Base08.px(density)))` |
| `Transform::from_xyz(_, _, 100.0)` | `Transform::from_xyz(_, _, ElevationIndex::ElevatedSurface.z())` |
| `TextFont { font_size: 12.0, .. }` 直書き | `theme.typography.label(LabelSize::Small).size` |

## 9. Bevy 0.18 固有の罠

- **Sprite tint は `Sprite.color = WHITE` 必須**。default `Sprite::default()` の color は使われず、tint したい場合は明示的に書く。
- **`Pickable::IGNORE` を持つ entity は背後をブロックする**。装飾用 Sprite に `Pickable::IGNORE` を付けると、その下にあるドラッグ可能要素にイベントが届かなくなる（M21 回帰ガード）。
- **DPI 2x で line_height が二重適用される**ことがある。`line_height` を `size * 1.2` 等の係数で組むときは、render_scale を考慮しないと Retina で行間が倍に見える。

## 10. 本 issue (#48) で先送りした Design decisions deferred

#48 のスコープは「token 基盤の確立 + footer のみ token 化」です。以下は意図的に先送り:

1. **ボタン寸法 / footer 高さ / anchor 0** … footer.rs の `Val::Px(34.0)` 等の数値は token 化スコープ外。component helper（`Button::new(...)` 型 API）に集約するのは **#46 component helper 課題**。
2. **`SyntaxColors` の syntect / tree-sitter 連携** … 構造体宣言のみ。実装は **#50**（`bevscode` 置換）。
3. **Typography `mono` の editor / gutter / 板への配線** … 宣言のみ。**#50**。
4. **`InputPhase` SystemSet 化と `bevy_cosmic_edit::InputSet` 79 件の整理** … `bevy_cosmic_edit` 自体が #50 で消えるため、#50 内で `bevscode` の input set にラップする際に同時導入。
5. **`order_panel.rs` 1,219 行の実コード分割** … `docs/ui-refactor-plan.md` に**計画のみ**。実装は **#46 Slice B**。
6. **`footer.rs` 以外（menu_bar / sidebar / order_panel / modify_modal / scenario_startup / strategy_editor_*）の token 化** … **#46**。
7. **`theme.layout.footer_h = 24.0` と footer.rs 生 `Val::Px(28.0)` の値の食い違い** … 将来統一の余地。
8. **Light theme 完成 / JSON ロード** … 将来。

## 11. footer で touch しなかったもの（明示）

#48 Step 9 の footer.rs token 化では、以下を**意図的に残しました**:

- `Val::Px(28.0)` — footer 自身の高さ。
- `Val::Px(0.0)` — sticky anchor 用。
- `Val::Px(34.0)` / `30.0` / `50.0` / `20.0` — transport / speed / mode toggle ボタン寸法。

これらはすべて **#46 component helper 課題**（`Button::transport()` / `Button::speed()` などの helper API）で集約します。token 化を先行させるとボタン helper の設計を縛ってしまうため、寸法だけは温存しました。
