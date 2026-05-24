# Strategy Editor は bevy_ui_text_input に載せ替える（Bevy 0.18 化に伴うエディタ依存の刷新）

_Status: accepted（2026-05-24）。関連: issue #35。bevscode 案・cosmic_edit port 案は却下（下記）。_

Strategy Editor / Startup フォームの自前 `bevy_cosmic_edit`（vendored fork `crates/bevy_cosmic_edit`、Bevy 0.15 ピン）を撤去し、プロジェクトを **Bevy 0.15 → 0.18 へ段階移行**したうえで、テキスト入力を **[`bevy_ui_text_input`](https://crates.io/crates/bevy_ui_text_input)** に置換する。

前提となった事実: `bevy_cosmic_edit` は upstream（StaffEngineer / Dimchikkk）が 2025-03 に archive 済みで **Bevy 0.15 が上限**。Bevy を 0.16 に上げた瞬間にコンパイル不能になるため、0.18 化には「fork を手 port する」か「別ライブラリへ置換する」かのいずれかが不可避。`bevy_ui_text_input` は Bevy 0.16/0.17/0.18 にそれぞれ v0.5 / v0.6 / v0.7 で追従しており、multiline・テキスト選択・undo/redo・縦横スクロールを built-in で持つ。ただし **シンタックスハイライト（リッチテキスト）と world UI を明示的に非サポート**。

## Considered Options

- **bevy_ui_text_input（採用）**: メンテされ各メジャーに追従。エディタの「全文 get/set・変更通知・focus・read-only・scroll」を満たせる。screen-space の Bevy UI 専用なので、現行の world-space floating editor（PanCam 追随・zoom-crisp 描画）とシンタックスハイライトは失う。このトレードオフを受容する（保守された依存と、置換コストの低さを優先）。
- **bevscode（GPU instanced text のコードエディタ）— 却下**: world-space 描画と tree-sitter ハイライト維持の魅力があったが、(a) world-space で instanced text を描けるか未検証で render pipeline の fork リスクがあり HITL spike が必要、(b) Bevy 0.18 のみ対応で **big-bang upgrade を強制**（段階移行で回帰を切り分けられない）、(c) LSP 同梱など依存が重い。当初 issue #35 はこの案だったが、リスクと big-bang 強制を避けるため取り下げた。
- **cosmic_edit fork を 0.18 へ手 port — 却下**: 既存 API・world-space・ハイライトを全維持できるが、archive 済みクレートの `render.rs` を Bevy の retained render world 変更（0.16〜）に 3 メジャー跨いで追従させる保守を恒久的に抱える。保守負債が大きい。

## Consequences

- **段階移行**: Bevy 0.16 → 0.17 → 0.18 を各メジャーで `cargo build` / `test` green にしてコミット。`bevy_cosmic_edit` は 0.16 で壊れるため、0.15→0.16 ステップで cosmic 撤去と `bevy_ui_text_input` 置換を**原子的**に行う。
- **失う機能（明示）**: シンタックスハイライト（`syntect` トークン着色・bracket/find ハイライト）、world-space 編集（PanCam 追随・zoom-crisp）、Tab→spaces / auto-indent / 括弧オートクローズ、自前の find/replace・スクロールバー。bevscode 案で予定していた tree-sitter-python ハイライトも採らない。
- **行番号ガター（追加 regression・計画の当初明示リスト外）**: `bevy_ui_text_input` に built-in 行番号ガターが無く、cosmic ベースの `strategy_editor_gutter.rs` は撤去される。静的な行番号列はスクロールで番号とコードがズレて誤誘導するため作らない。スクロール同期ガターは内部 scroll state への結合（不確実 API）が必要で、本 ADR が却下した「エディタ内部への壊れやすい結合」の再来になるため本移行には含めない。**Python 戦略の traceback 行番号とコードの突き合わせ**が重要になった場合のみ、scroll offset の公開を確認したうえで後続 issue として検討する。
- **撤去する依存**: `bevy_cosmic_edit`（vendored fork `crates/bevy_cosmic_edit` + `[patch.crates-io]`）、dev-dep `cosmic-text`、`syntect`（+ `examples/syntect_smoke.rs`）。
- **配置の変化**: Strategy Editor は world-space floating window → **screen-space Bevy UI** になる。`CONTEXT.md` の「Strategy Editor hosts a `cosmic-edit` buffer」記述（現行コードに対しては正確）は、**エディタ移行時に `bevy_ui_text_input`・screen-space を反映して更新**する（実装前に書き換えると未実装状態を記述してしまうため、移行と同時に行う）。
- **screen-space ホスティング方式（2026-05-24 決定）**: editor と Startup は world-space sprite floating window をやめ、**screen-space の draggable floating panel**（absolute 配置の Bevy UI `Node` + ドラッグ可能なタイトルバー + `GlobalZIndex` 前面化）に作り替え、その中に `bevy_ui_text_input` の `TextInputNode` をホストする。「無限キャンバス上の windows」という UI アイデンティティを保つため fixed/docked ではなく draggable を採用（lightweight より UX 一貫性を優先）。位置永続化は world xy → UI node の `left`/`top`。影響: m2(drag)/m4(focus z)/m9(startup pos) を UI-node セマンティクスへ re-point。Startup の hard invariant（× ボタン無し・Replay 限定表示）は維持。他の表示専用パネル（buying_power/chart 等）は world-space sprite のまま（2 流派併存は既存どおり）。
- **E2E / wiki**: drop した editor 機能の flow（j2/j3/j4/j5/j6 + highlight 系）は撤去 or 緩和し `FLOWS.md` を更新。cosmic を headless fixture に使う flow（i5/i17/i18/m1/m5/m12/j1 等）を新エディタ化。`docs/wiki/strategy.md` を現行化し `[FlowID]` を引く（`behavior-to-e2e` スキル）。
- 依存マトリクス・破壊的変更チェックリストなど詳細手順は **issue #35** を参照。

## 既知の follow-up（A-2 レビューで検出・#35 で closeする）

A-2 完了レビュー（2026-05-24）で、screen-space 化に伴う以下の積み残しを検出。いずれも「新コードのバグ」ではなく **screen-space window 種別が既存機構の対象外**であることに起因する。upgrade（Step B/C）完了までに closeする。

- **layout 永続化が screen window 未対応（中〜高）**: `layout_persistence` の `build_layout` / `apply_layout_system` は world-space の `&Sprite`+`&Transform` window のみ save/restore する。`ScreenWindowRoot`（Strategy Editor / Startup、Node の left/top/width/height）は対象外なので **位置・サイズ・可視性がサイドカーへ永続化されない**（上の「位置永続化は world xy → UI node の left/top」が未実装）。`ScreenWindowRoot` 分岐（Node geometry の read/write + restore 時の `Node.left/top` 書き込み）の追加が残作業。`m9` flow はテストが Sprite 偽装 startup root を使うため production gap を捕捉していない（fix と同時に m9 を実 screen window へ re-point する）。
- **× クローズが undo 不可（低〜中）**: 旧 world-space editor の close は `AppHistory.push_window_despawn`（テキスト snapshot 付き）を積み Ctrl+Z で復活できた。`screen_window` の汎用 close observer は `despawn` + autosave dirty のみで history を積まない。editor 専用の close-undo が要るなら別途配線（editor テキストは cache へ debounce 保存されるので消失はしない）。
- **editor 上のホイールが world camera もズーム（中）**: 旧 `camera.rs` は editor sprite 上で PanCam を抑制したが、screen-space 化で editor 分岐を撤去。PanCam は raw `MouseWheel` を UI 非認識で消費するため、editor 上スクロールで背後 camera も同時ズームし得る。chart 分岐のような UI-aware 抑制が要る。
- **suppress_echo_target は単一 Option**: multi-region editor で undo 時に全 editor を再 Paste するが echo 抑制は 1 region 分のみ。現状は `fragment.source == new_text` の byte 一致 fallback で守られているが、正規化差や filter/max_chars 起因の Paste 不一致で stuck/誤消費し得る（per-region 化が望ましい）。
