# UI Theme & Design Tokens (#48)

本ドキュメントは `src/ui/theme/` 配下で定義されるデザイントークン群（Theme / ColorScale / DynamicSpacing / Typography / Elevation）の運用ガイドです。新しい UI コードを書くときに参照してください。

## 1. 概要

- `Theme` Bevy `Resource` が dark variant の全トークンを保持します（`src/ui/mod.rs` で `app.add_plugins(theme::ThemePlugin)`。`ThemePlugin` が内部で `init_resource::<Theme>()` を呼ぶ単一窓口です — `init_resource::<Theme>()` を呼び出し側で直書きしないこと）。
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

呼び出し規約: 実コードでは `theme.spacing.px(DynamicSpacing::Base08)` 経由で引いてください。`DynamicSpacing::Base08.px(density)` を直接呼ぶことは禁止（density 引数を call site に漏らさないため）。`SpacingTokens` wrapper が density を保持しているので、call site は variant だけ渡せば済みます（#48 M5）。

## 5. `Typography`

`src/ui/theme/typography.rs`。`HeadlineSize {XSmall, Small, Medium, Large, XLarge}` と `LabelSize {XSmall, Small, Default, Large}` の 2 軸。

- **heading** … パネルタイトル / モーダル見出し。`theme.typography.headline(HeadlineSize::Small).size` のように引きます。
- **label** … footer / button / 小さなメタ情報。footer の text_font はすべて `LabelSize::XSmall` / `Small` / `Default` に置換済みです。
- **body** … 本文・長文。
- **mono** … 等幅。**#48 では宣言のみ**。editor / gutter / 板への配線は #50（`bevscode` 置換）で行います。

## 6. `ElevationIndex`

`src/ui/theme/elevation.rs`。`Transform.translation.z` の直書きを根絶するための tier:

| Variant | z | `background(theme)` 戻り値 | 用途 |
|---|---|---|---|
| `Background` | 0 | `theme.colors.background` | root 背景 |
| `Surface` | 10 | `theme.colors.surface_background` | footer / sidebar / menu / 通常パネル |
| `ElevatedSurface` | 100 | `theme.colors.elevated_surface_background` | popover / dropdown / tooltip |
| `ModalSurface` | 300 | `theme.colors.elevated_surface_background` | モーダルダイアログ |
| `Notification` | 500 | `theme.colors.elevated_surface_background` | toast / safety rail violation |
| `DragOverlay` | 700 | `theme.colors.elevated_surface_background` | drag preview |

裁定: modal は `ModalSurface`、toast は `Notification`、popover は `ElevatedSurface` を使ってください。`background(theme)` は `ElevationIndex` から `ThemeColors` フィールドへの透過 lookup を提供します（z 値と背景色を同じ tier 概念で引けるようにするため）。

呼び出し規約: 実コードでは `theme.elevation.background(ElevationIndex::Surface)` 経由で引いてください。`ElevationIndex::background_for_colors(&theme.colors)` を直接呼ぶことは禁止（`ElevationTokens` wrapper が `&Theme` を保持しているので call site は tier だけ渡せば済む）。

## 7. `Radius` / `Layout` / `Appearance`

- `Radius { sm, md, lg, full }` — `border_radius` 用。
- `Layout { toolbar_h, footer_h, sidebar_w, inspector_w, footer_button_h, footer_transport_button_w, footer_speed_button_w, footer_mode_button_w }` — UI レイアウトの基本寸法（footer 寸法 4 つは #48 H6 で追加）。UI density は `SpacingTokens::density`（`theme.spacing.density`）が single source of truth で、`Layout` 側には持たせません。`footer_h` は 28.0 に統一済み（旧 24/28 食い違いは H6 で解消）。
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

#48 のスコープは「token 基盤の確立 + footer のみ token 化」です。

### 10.0 本 #48 セッションで実装済み（B / E / F / O）

以下の AC 項目は本 `refac/#48-step0` セッションの Slice 1–4b で実装されました:

- **F**: `ElevationIndex::background(&Theme) -> Color` 実装済み（`src/ui/theme/elevation.rs`、Slice 1）。z 値と並ぶ tier → 背景色 lookup を提供。
- **E**: `TypeStyle::text_font() -> (TextFont, LineHeight)` および `Typography::label_font(LabelSize) / headline_font(HeadlineSize)` helper 実装済み（Slice 2）。Bevy 0.18 で `TextFont.line_height` field が廃止され `LineHeight` が独立 Component になった都合上、tuple-of-Components 返しを採用。footer.rs の `TextFont { font_size, ..default() }` 9 箇所が helper 経由に置換済み。
- **B**: `Theme` に `scale: ColorScales` / `spacing: SpacingTokens` / `elevation: ElevationTokens` フィールドを追加し、`Layout::density` を `SpacingTokens::density` へ一本化済み（Slice 3）。
- **O**: `Theme` 配下の struct/enum に `serde::Serialize` / `serde::Deserialize` derive を追加し、`tests/e2e/flows/q3_theme_serde_roundtrip.rs` で `Theme::default()` の JSON round-trip ガードを追加済み（Slice 4 / 4b）。

### 10.1 引き続き先送り

以下は意図的に先送り:

1. **ボタン寸法 / footer 高さ / anchor 0** … footer.rs の `Val::Px(34.0)` 等の数値は token 化スコープ外。component helper（`Button::new(...)` 型 API）に集約するのは **#46 component helper 課題**。
2. **`SyntaxColors` の syntect / tree-sitter 連携** … 構造体宣言のみ。実装は **#50**（`bevscode` 置換）。
3. **Typography `mono` の editor / gutter / 板への配線** … 宣言のみ。**#50**。
4. **`InputPhase` SystemSet 化** … **#46 Slice A で最小導入済み**（`src/ui/input_phase.rs`:
   `InputPhase::{KeyboardDrain, ModalInput, WidgetInput, CosmicEdit}` を `.chain()` 順序固定）。
   現状メンバは `WidgetInput`（`button_interaction_system`）のみ。`KeyboardDrain` は #46 Slice E、
   `ModalInput` は #46 Slice B、`CosmicEdit`（bevscode 連携）は #50 で投入。#50 はこの set を
   **再定義せず流用**する。
5. **`order_panel.rs` 1,219 行の実コード分割** … `docs/ui-refactor-plan.md` に**計画のみ**。実装は **#46 Slice B**。
6. **`footer.rs` 以外（menu_bar / sidebar / order_panel / modify_modal / scenario_startup / strategy_editor_*）の token 化** … **#46**。
7. **Light theme 完成 / JSON ロード** … 将来。
8. **`strategy_editor.rs` の token 化** … #50 内で `bevscode` 上に乗ったタイミングで実施（#48 範囲外、`Color::srgba` 直書き残存は観測のみ。`strategy_editor_spike.rs` は #50 Slice 7 で撤去済み）。

## 11. footer 寸法 token（#48 H6 で完全 token 化）

`docs/ui-theme.md` 旧 §11 の carve-out（`Val::Px(28.0)` / `34.0` / `30.0` / `50.0` / `20.0` を #46 まで温存）は **#48 H6 で撤回** しました。footer.rs は raw `Val::Px(<数値>)` を持たず、すべて `theme.layout.*` 経由です:

- `theme.layout.footer_h` (28.0) — footer 高さ
- `theme.layout.footer_button_h` (20.0) — 全 footer ボタン共通高さ
- `theme.layout.footer_transport_button_w` (34.0) — transport ボタン幅
- `theme.layout.footer_speed_button_w` (30.0) — speed ボタン幅
- `theme.layout.footer_mode_button_w` (50.0) — ExecutionMode toggle 幅

sticky anchor (`bottom/left/right = 0`) は design token ではなく positional constant なので `Val::ZERO` を使います。

#46 の component helper（`Button::transport()` 等）は、本 token を default 値として参照するラッパとして後日設計します。token 自体は本 issue で確定。

## 12. Button component (#46 Slice A)

`src/ui/component/button.rs` が、散在していたボタン色変化 system（footer / menu_bar /
sidebar / live_run / modify_modal の `Changed<Interaction>` / resource 駆動の色分岐）を
**単一の `button_interaction_system` + `ButtonStyle × ButtonState` テーブル**に集約します。
ボタンに `ButtonStyle` を付けるだけで、hover / press / selected / disabled の色が
`Theme` から自動解決されます。

### 利用例（builder）

```rust
use crate::ui::component::{spawn_button, ButtonStyle, TintColor};
use crate::ui::traits::{Clickable, Disableable, Toggleable, UiSized, UiStyledExt};
use crate::ui::theme::ElevationIndex;
use crate::ui::traits::ComponentSize;

spawn_button(&mut commands, &theme, "Run")
    .style(ButtonStyle::Tinted(TintColor::Success))
    .size(ComponentSize::Default)
    .elevation(ElevationIndex::Surface)
    .on_click(|_commands| info!("run clicked"));   // FnMut(&mut Commands) closure → On<Pointer<Click>> observer
```

既存ボタンの移行は spawn タプルに `ButtonStyle` + `ElevationIndex` を足し、各 system から
色代入を剥がすだけ。「選択中」「無効」状態はマーカー component で表す:

- `ButtonSelected` — トグル ON / 現在値（speed の現在倍率・ExecutionMode の現在セグメント等）。
  action system が `commands.insert/remove` で切替え、generic system が `Selected` 色を塗る。
- `ButtonDisabled` — 無効状態（venue busy・confirm 不可・live run の不許可 action 等）。
  `Disabled` が他のどの状態より優先される。

### `ButtonStyle × ButtonState` テーブル（dark）

`button_colors(style, state, elevation, &theme)` が唯一の解決点。`label` 列に応じて `icon`
列も同じ階調（`icon` / `icon_muted` / `icon_disabled` / `icon_accent`）になる。

| style \ state | Enabled (bg / border / label) | Hovered | Active | Selected | Disabled |
|---|---|---|---|---|---|
| **Filled** | element_background / NONE / text | element_hover | element_active | element_selected / border_selected | element_disabled / border_disabled / text_disabled |
| **Tinted(t)** | tint_bg / tint_border / tint_label | tint_solid | tint_solid | tint_solid | element_disabled / border_disabled / text_disabled |
| **Outlined** | NONE / border / text | element_hover / border_variant | element_active / border_variant | element_selected / border_selected | NONE / border_disabled / text_disabled |
| **OutlinedGhost** | NONE / border / text_muted | ghost_element_hover | ghost_element_active | ghost_element_selected / border_selected | NONE / border_disabled / text_disabled |
| **Subtle** | ghost_element_background / NONE / text_muted | ghost_element_hover | ghost_element_active | ghost_element_selected | ghost_element_disabled / NONE / text_disabled |
| **Transparent** | NONE / NONE / text_muted | ghost_element_hover | ghost_element_active | ghost_element_selected | NONE / NONE / text_disabled |

- `TintColor::{Accent, Error, Warning, Success}` は `theme.status.*`（error/warning/success）
  と accent トークン（Accent）に解決される。Submit=`Tinted(Success)`、Cancel/Stop=`Tinted(Error)`。
- `Focused` 行は将来の focus-ring slice 用に予約（Slice A では生成しない）。
- builder は `#48` trait ピラミッド（`Clickable` / `Disableable` / `Toggleable` / `UiSized` /
  `UiStyled` / `UiStyledExt`）を impl 済み。`button_interaction_system` は
  `InputPhase::WidgetInput` set に登録される（§10.1-4 参照）。

### Slice A の対象外（後続スライス）

- **静的色のままのボタン**（secret / reconcile / relogin / settings / instrument_picker /
  strategy_editor_find の発注確認以外）… 色変化 system を持たず spawn 時固定色のため、
  生値ゼロ化は **Slice H**（残存生値ゼロ化 + CI 機械検査）で実施。
- **`order_context_menu` の hover**（`context_menu_hover_system` の cyan ハイライト）… 既存の
  distinctive な cyan（`COLOR_ITEM_HOVER`）に対応する theme トークンが無く、`ButtonStyle` 化すると
  色味が変わるため Slice A では現状維持。token 化は **Slice H**。
- **`order_panel` の confirm / submit ボタン** … world-space Sprite + observer 方式で UI-Node の
  `button_interaction_system` の対象外。**Slice B**（`order_panel` 分割 + `ModalSkeleton`）で扱う。

## 12.5 Modal component (#46 Slice B)

`src/ui/component/modal_layer.rs` が、各モーダルが個別に持っていた spawn コードと Escape 消化 system を、**単一の `ModalLayer` スタック + 汎用 `modal_layer_esc_system` + `spawn_modal` スケルトン**に集約します。Button(§12) が「色変化 system の集約」だったのに対し、Modal は「スタック管理と dismiss 経路の集約」が主眼です。

### `ModalLayer` スタック

`ModalLayer`（Bevy `Resource`）は開いているモーダルの LIFO スタック（`Vec<ActiveModal>`、末尾が frontmost）を持ちます。

- `push(ActiveModal)` — 新しいモーダルを frontmost として積む。
- `pop() -> Option<ActiveModal>` — frontmost を取り出す。
- `try_dismiss_top() -> bool` — frontmost の `on_before_dismiss` を引いて、`DismissDecision::Dismiss` のときだけ pop し `true` を返す。`DismissDecision::Pending`（処理中などで dismiss を拒否）なら積んだまま `false`。

`ActiveModal { root, backdrop, previous_focus, on_before_dismiss }`:

- `root` / `backdrop` … モーダル本体と背後のバックドロップ entity。
- `previous_focus: Option<Entity>` … モーダルを開く前に focus を持っていた entity。**本パスでは記録のみ（record-only）** で、復元はしません（グローバル focus リソースが未導入のため）。
- `on_before_dismiss: fn() -> DismissDecision` … dismiss 前に引かれる veto フック。

### Escape での dismiss（`modal_layer_esc_system`）

`modal_layer_esc_system` が Escape を消化し、frontmost モーダルを `try_dismiss_top` で閉じます。ただし、より優先度の高い入力モーダル（secret / order-confirm / modify）が開いているときは Esc を譲ります（`esc_yield_clear(secret_active, confirm_pending, modify_open)` が `false` のとき早期 return）。スタックが空 / Escape 未押下のときも no-op。本 system は relogin 通知の旧 yield ガードと同じ「一度の Escape を二重消費させない」契約を引き継ぎます。

### `ModalSkeleton` / `spawn_modal`

```rust
use crate::ui::component::modal_layer::{spawn_modal, ModalSkeleton, ModalHandle};

let ModalHandle { root, card } = spawn_modal(
    &mut commands,
    &theme,
    ModalSkeleton { width: 360.0, z_index: 260, name: "Relogin" },
);
// `root` は full-screen バックドロップ（spawn 時 `Display::None`）、
// `card` は中央寄せの `ElevationIndex::ModalSurface` サーフェス。
// 中身（テキスト・ボタン）は呼び出し側が `card` の子として足す。
```

`spawn_modal` が組むもの:

- `card` … `width` 指定・`padding = DynamicSpacing::Base16`・`BackgroundColor = ElevationIndex::ModalSurface.background(theme)`・`ElevationIndex::ModalSurface` 付き（生値ゼロ）。
- `root`（バックドロップ）… full-screen・spawn 時 `Display::None`・`BackgroundColor = theme.colors.background.with_alpha(0.6)`・`GlobalZIndex(z_index)`・`Name`。`card` を子に持つ。

`z_index` は per-modal の重なり（relogin 260 / reconcile 262 …）を当面そのまま保持します（`ElevationIndex` が z を完全に所有するまでの暫定）。

### 移行状況

- **relogin / reconcile モーダルは本パス（Slice B）で移行済み**：spawn は `spawn_modal` + theme トークン（生値ゼロ）に組み直し、Escape dismiss は汎用 `modal_layer_esc_system` を通ります（per-modal の双方向 stack↔trigger 同期 system を併設）。各モーダル固有の system には Close/confirm クリックだけが残ります。観測可能挙動の回帰ガードは `[k13]`（relogin Esc 優先）/ `[k14]`（reconcile Esc 優先）、`ModalLayer` 基盤そのものは `modal_layer.rs` の in-src ユニットテスト `m_modal_01..09` が担保します。
- **secret / modify / order-confirm モーダルは本パス未移行**（まだ legacy spawn / 個別 Esc 経路）。`previous_focus` の復元もグローバル focus リソース導入後の後続スライスで扱います。

## 13. Issue #48 review followup（fix/#48-review-followup ブランチ）

#48 マージ後の Navigator + codex レビューで挙がった Medium 以上の指摘 13 件に対応したセッションで以下が landed しました。本文の他章はこの変更を反映済みなので、差分の history としてのみ記載します:

- **M2**: `DynamicSpacing` に `Base01` / `Base03` / `Base10` variant を追加（spec §4 で言及されていたが欠落していた 3 step）。
- **H1 + H2**: `accent_dark` (Radix iris) / `red_dark` / `green_dark` (Radix grass) / `yellow_dark` (Radix amber) / `blue_dark` を Radix 12-step フル値で埋める（旧実装は step 9/11/12 だけが hue 固有、残りは neutral 流用だった）。これにより `accent` と `blue` (info) が同一 RGB だった視覚衝突も解消。
- **H3**: `ThemeColors::from_scales(&ColorScales)` / `StatusColors::from_scales` / `SyntaxColors::from_scales` / `PlayerColors::from_scales` / `Theme::from_scales(ColorScales)` に refactor。`Theme::default()` は scale 再構築を二重持ちせず `Self::from_scales(ColorScales::default())` に集約。AC §8「`ColorScales` 差し替えで Light を組める」契約を回復。
- **H7**: Zed-style `ThemeColors` 役割フィールド 29 件追加（`panel_focused_border`, `ghost_element_{background,hover,active,selected,disabled}`, `border_{selected,disabled,transparent}`, `element_{disabled,selection_background}`, `drop_target_{background,border}`, `status_bar_background`, `title_bar_background`, `toolbar_background`, `tab_bar_background`, `tab_{active,inactive}_background`, `search_{match,active_match}_background`, `scrollbar_thumb_{background,hover_background,active_background}`, `scrollbar_track_background`, `gutter_background`, `line_{number,number_active}`, `icon_placeholder`）。すべて per-field `///` doc で `(scale.X.step_N)` を引用。
- **H4**: `ElevationIndex` の `ModalSurface` / `Notification` / `DragOverlay` / `ElevatedSurface` が同じ `elevated_surface_background` に潰れていた問題を解消。`modal_background` / `notification_background` / `drag_overlay_background` を `ThemeColors` に追加し、`background_for_colors` を tier 別 routing に書き換え。
- **M7**: `Theme::dark()` / `Theme::light()` constructors + `ColorScales::dark()` / `ColorScales::light()`。`Default for Theme` は `Self::dark()` に。`Light` palette 本体は `ColorScales::light() == Self::dark()` の stub（実 Light scale 値は本 issue 範囲外）。
- **M5**: footer.rs 4 箇所を `DynamicSpacing::Base<N>.px(theme.spacing.density)` 直叩きから `theme.spacing.px(DynamicSpacing::Base<N>)` wrapper 経由に統一。
- **H6**: footer.rs を完全 token 化。raw `Val::Px(34/30/50/20/28/0)` を `theme.layout.footer_{transport,speed,mode}_button_w` / `footer_button_h` / `footer_h` / `Val::ZERO` に置換。docs §11 の carve-out を撤回。`Layout::default().footer_h` を 24.0 → 28.0 に bump。Q2 lint flow が raw `Val::Px(<数値>)` を ban するよう強化。
- **H5**: `InputPhase { KeyboardDrain, ModalInput, WidgetInput }` SystemSet を `src/ui/mod.rs` に定義し、`UiPlugin` で `.chain()` 設定。代表 2 system (`secret_modal_input_system` → `ModalInput`, `menu_keyboard_system` → `WidgetInput`) を migrate。残り systems は doc TODO で段階移行リスト化。
- **M6**: `Clickable::on_click` を `FnMut()` から `FnMut(&mut Commands)` ベースに再設計。`OnClick(Box<dyn FnMut(&mut Commands) + Send + Sync>)` Component を追加。#46 helper が `Commands::send` / `MessageWriter::write` を closure 内で実行できるように。
- **M1**: `q3_theme_serde_roundtrip_non_default_fields` を comprehensive fixture に置換。`backcast::ui::theme::non_default_theme()` で全 sub-struct の全 serializable field を non-default 値に mutate し、`#[serde(skip)]` 混入を PartialEq round-trip assert で gate。private `Typography.headline` / `Typography.label` は同 module 内の `non_default_typography()` helper で生成。両 helper は integration test target が `cfg(test)` を継承しない制約のため `#[doc(hidden)] pub fn` で公開。
- **M3**: `tests/e2e/FLOWS.md` への Q3 entry（既に landed）。
- **M4**: 本 §13 を含む docs/ui-theme.md drift 修正（density は SpacingTokens、ThemePlugin 経由 init、wrapper-as-sole-API 規約）。
- **wrapper-as-sole-API**: `DynamicSpacing::px(density)` / `ElevationIndex::background_for_colors(&ThemeColors)` の直接呼び出しを禁止し、`theme.spacing.px(...)` / `theme.elevation.background(...)` を唯一の窓口に統一（§4 / §6 参照）。
