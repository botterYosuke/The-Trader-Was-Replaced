//! Undo/Redo 単一タイムライン管理モジュール。
//!
//! `undo` crate v0.52 の `Record<AppEdit>` を使い、テキスト編集・ウィンドウドラッグ・
//! パネル spawn/despawn を Ctrl+Z / Ctrl+Y で巻き戻し・やり直しできるようにする。
//!
//! # 設計の要点
//! - `AppEdit::edit/undo` は ECS ミューテーションを**直接行わず**、
//!   `PendingAppEdits` にアクションを push するだけ。
//! - 実際の ECS ミューテーション（Commands / Query）は翌フレームで
//!   `apply_pending_app_edits_system` が処理する。
//! - `AppHistory::replaying_depth > 0` の間は push を抑止することで
//!   undo/redo の適用中に新たなエントリが積まれないようにする。

use bevy::prelude::*;
use std::collections::VecDeque;
use std::time::Instant;
use undo::{Edit, Merged, Record};

use crate::ui::components::PanelKind;
use crate::ui::layout_persistence::WindowLayout;

// ─────────────────────────────────────────────────────────────────────────────
// PendingAppEdits — ECS ミューテーションキュー（Edit::Target）
// ─────────────────────────────────────────────────────────────────────────────

/// undo/redo の apply/undo 側が積む ECS アクション。
/// `apply_pending_app_edits_system` が翌フレームで drain して ECS に反映する。
#[derive(Debug, Clone)]
pub enum AppEditAction {
    /// Strategy Editor のテキストを指定文字列に置き換える。
    SetStrategySource { region_key: String, text: String },
    /// 指定 kind のウィンドウを指定位置に移動する（PanelKind で検索するため entity 不要）。
    MoveWindow {
        kind: PanelKind,
        region_key: Option<String>,
        position: Vec2,
    },
    /// 指定 layout のパネルを spawn する（undo 時の WindowDespawn の逆など）。
    /// layout に位置・サイズ・z が含まれるため、復元位置が正確になる。
    SpawnWindow {
        layout: WindowLayout,
        strategy_snapshot: Option<(String, String)>,
    },
    /// 指定 kind のパネルを despawn する（PanelKind で検索するため entity 不要）。
    DespawnWindow {
        kind: PanelKind,
        region_key: Option<String>,
    },
}

/// `Edit::Target` として `Record` に渡す pending キュー。
/// `apply_pending_app_edits_system` が毎フレーム drain する。
#[derive(Default, Debug)]
pub struct PendingAppEdits {
    pub queue: VecDeque<AppEditAction>,
}

impl PendingAppEdits {
    pub fn push(&mut self, action: AppEditAction) {
        self.queue.push_back(action);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AppEdit — Edit コマンド enum
// ─────────────────────────────────────────────────────────────────────────────

/// テキスト編集の before/after を保持するコマンド。
#[derive(Debug, Clone)]
pub struct TextEdit {
    /// このテキスト編集が属する StrategyEditor の region_key。
    pub region_key: String,
    pub before: String,
    pub after: String,
    /// 直前の TextEdit からの経過時間（マージ判定に使う）。
    pub timestamp: Instant,
}

/// ウィンドウのドラッグ移動の before/after を保持するコマンド。
#[derive(Debug, Clone)]
pub struct WindowMoveEdit {
    pub kind: PanelKind,
    /// StrategyEditor の場合に使う region 特定キー。Chart 等は None。
    pub region_key: Option<String>,
    pub before: Vec2,
    pub after: Vec2,
}

/// パネルを開いたコマンド（undo で despawn）。
/// redo 時はデフォルト位置に spawn する（layout は dispatcher が設定する仮値）。
#[derive(Debug, Clone)]
pub struct WindowSpawnEdit {
    pub kind: PanelKind,
    /// redo 時の再 spawn 位置（dispatcher が渡すデフォルト値）。
    pub layout: WindowLayout,
}

/// パネルを閉じたコマンド（undo で spawn）。
#[derive(Debug, Clone)]
pub struct WindowDespawnEdit {
    /// 閉じた瞬間の完全なレイアウトスナップショット（kind・position・size・z を含む）。
    /// undo 時にこの位置・サイズ・z でパネルを再 spawn する。
    pub layout: WindowLayout,
    /// (region_key, source) のタプル。undo で Strategy Editor を再 spawn するときに両方必要。
    pub strategy_snapshot: Option<(String, String)>,
}

/// Undo/Redo スタックに積む編集コマンドの種別。
#[derive(Debug, Clone)]
pub enum AppEdit {
    Text(TextEdit),
    WindowMove(WindowMoveEdit),
    WindowSpawn(WindowSpawnEdit),
    WindowDespawn(WindowDespawnEdit),
}

// ─────────────────────────────────────────────────────────────────────────────
// Edit トレイト実装
// ─────────────────────────────────────────────────────────────────────────────

/// テキスト編集のマージ判定で使う定数。
/// 500ms 以内・差分 50 文字以下・改行追加なし → マージ
const TEXT_MERGE_WINDOW_MS: u128 = 500;
const TEXT_MERGE_MAX_DIFF: usize = 50;

impl Edit for AppEdit {
    type Target = PendingAppEdits;
    /// `()` — エラー処理は apply_pending_app_edits_system 側で行う。
    type Output = ();

    /// redo / 初回 apply: pending に「適用アクション」を push するだけ。
    fn edit(&mut self, target: &mut PendingAppEdits) {
        match self {
            AppEdit::Text(t) => {
                target.push(AppEditAction::SetStrategySource {
                    region_key: t.region_key.clone(),
                    text: t.after.clone(),
                });
            }
            AppEdit::WindowMove(w) => {
                target.push(AppEditAction::MoveWindow {
                    kind: w.kind,
                    region_key: w.region_key.clone(),
                    position: w.after,
                });
            }
            AppEdit::WindowSpawn(s) => {
                // redo: 再 spawn する（デフォルト位置の layout を持つ）
                target.push(AppEditAction::SpawnWindow {
                    layout: s.layout.clone(),
                    strategy_snapshot: None,
                });
            }
            AppEdit::WindowDespawn(d) => {
                target.push(AppEditAction::DespawnWindow {
                    kind: d.layout.kind,
                    region_key: d.layout.region_key.clone(),
                });
            }
        }
    }

    /// undo: pending に「逆アクション」を push するだけ。
    fn undo(&mut self, target: &mut PendingAppEdits) {
        match self {
            AppEdit::Text(t) => {
                target.push(AppEditAction::SetStrategySource {
                    region_key: t.region_key.clone(),
                    text: t.before.clone(),
                });
            }
            AppEdit::WindowMove(w) => {
                target.push(AppEditAction::MoveWindow {
                    kind: w.kind,
                    region_key: w.region_key.clone(),
                    position: w.before,
                });
            }
            AppEdit::WindowSpawn(s) => {
                // spawn を undo → despawn（PanelKind で検索）
                target.push(AppEditAction::DespawnWindow {
                    kind: s.kind,
                    region_key: s.layout.region_key.clone(),
                });
            }
            AppEdit::WindowDespawn(d) => {
                // despawn を undo → spawn 再現（閉じた瞬間の layout + snapshot を渡す）
                target.push(AppEditAction::SpawnWindow {
                    layout: d.layout.clone(),
                    strategy_snapshot: d.strategy_snapshot.clone(),
                });
            }
        }
    }

    /// Text 同士のマージポリシー:
    /// 500ms 以内 & 差分 50 文字以下 & \n 追加なし → Merged::Yes
    fn merge(&mut self, other: Self) -> Merged<Self> {
        if let (AppEdit::Text(self_t), AppEdit::Text(other_t)) = (self, &other) {
            let elapsed = other_t
                .timestamp
                .duration_since(self_t.timestamp)
                .as_millis();
            let diff_len = other_t.after.len().abs_diff(self_t.after.len());
            let added_newline = other_t.after.contains('\n') && !self_t.after.contains('\n');

            if elapsed <= TEXT_MERGE_WINDOW_MS && diff_len <= TEXT_MERGE_MAX_DIFF && !added_newline
            {
                // after を更新してマージ。timestamp はそのまま（最初のキーストロークを基準にする）
                self_t.after = other_t.after.clone();
                return Merged::Yes;
            }
        }
        Merged::No(other)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AppHistory Resource
// ─────────────────────────────────────────────────────────────────────────────

/// Undo/Redo の履歴を管理する Bevy Resource。
///
/// - `record`: `undo` crate の `Record<AppEdit>`。履歴スタック本体。
/// - `pending`: ECS ミューテーションキュー。`apply_pending_app_edits_system` が drain する。
/// - `replaying_depth`: > 0 の間は新たなエントリ push を抑止する。
///   `undo_redo_system` が undo/redo 呼び出し前に +1、
///   `apply_pending_app_edits_system` が drain 完了後に -1 する。
/// - `suppress_echo_target`: `Some(text)` のとき `sync_editor_to_strategy_buffer_system` が
///   bevscode からの text change イベント内容を `text` と比較し、一致した場合のみ無視（消費）する。
///   undo/redo 適用後に bevscode が返す echo を「期待テキスト一致」で判別することで、
///   echo が 1 回しか来なかった場合でも次の本物のユーザー入力を誤って捨てない。
#[derive(Resource)]
pub struct AppHistory {
    pub record: Record<AppEdit>,
    pub pending: PendingAppEdits,
    pub replaying_depth: u32,
    pub suppress_echo_target: Option<(String, String)>,
}

impl Default for AppHistory {
    fn default() -> Self {
        Self {
            // 履歴上限 200 件。超過した古いエントリは自動的に削除される。
            record: Record::builder().limit(200).build(),
            pending: PendingAppEdits::default(),
            replaying_depth: 0,
            suppress_echo_target: None,
        }
    }
}

impl AppHistory {
    /// undo/redo 実行中かどうかを返す。
    /// `true` の間は新たなエントリを Record に push しない。
    pub fn is_replaying(&self) -> bool {
        self.replaying_depth > 0
    }

    /// undo/redo や外部からのテキスト適用後に bevscode が echo する
    /// text change イベントを無視するターゲットテキストをセットする。
    /// `sync_editor_to_strategy_buffer_system` が `new_text == target` のとき
    /// だけ echo を消費・無視し、異なるテキストが来た場合はターゲットをクリアして
    /// 通常入力として履歴に積む。
    pub fn suppress_echo(&mut self, region_key: String, text: String) {
        self.suppress_echo_target = Some((region_key, text));
    }

    /// TextEdit を Record に push する。
    /// `is_replaying()` が true のときは何もしない。
    pub fn push_text(&mut self, region_key: String, before: String, after: String) {
        if self.is_replaying() {
            return;
        }
        let edit = AppEdit::Text(TextEdit {
            region_key,
            before,
            after,
            timestamp: Instant::now(),
        });
        self.record.edit(&mut self.pending, edit);
        // edit() で pending に push されたが、通常入力時のテキスト push は
        // apply_pending_app_edits_system ではなく sync_editor_to_strategy_buffer_system が
        // 直接 buffer を書き換えるため、ここで生成された pending アクションは drain しておく。
        // （redo 実行時のみ pending を使う）
        self.pending.queue.clear();
    }

    /// WindowMoveEdit を Record に push する。
    pub fn push_window_move(
        &mut self,
        kind: PanelKind,
        region_key: Option<String>,
        before: Vec2,
        after: Vec2,
    ) {
        if self.is_replaying() {
            return;
        }
        if (before - after).length() < 1.0 {
            return; // 誤差程度の移動は無視
        }
        let edit = AppEdit::WindowMove(WindowMoveEdit {
            kind,
            region_key,
            before,
            after,
        });
        self.record.edit(&mut self.pending, edit);
        self.pending.queue.clear();
    }

    /// WindowSpawnEdit を Record に push する。
    /// `layout` はユーザーが開いたときのデフォルト位置を表す仮値
    /// （redo 時はデフォルト位置に再 spawn される）。
    pub fn push_window_spawn(&mut self, kind: PanelKind, layout: WindowLayout) {
        if self.is_replaying() {
            return;
        }
        let edit = AppEdit::WindowSpawn(WindowSpawnEdit { kind, layout });
        self.record.edit(&mut self.pending, edit);
        self.pending.queue.clear();
    }

    /// WindowDespawnEdit を Record に push する。
    /// `layout` は閉じた瞬間の完全スナップショット（position・size・z を含む）。
    /// undo 時にこの位置でパネルを再 spawn する。
    pub fn push_window_despawn(
        &mut self,
        layout: WindowLayout,
        strategy_snapshot: Option<(String, String)>,
    ) {
        if self.is_replaying() {
            return;
        }
        let edit = AppEdit::WindowDespawn(WindowDespawnEdit {
            layout,
            strategy_snapshot,
        });
        self.record.edit(&mut self.pending, edit);
        self.pending.queue.clear();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Events
// ─────────────────────────────────────────────────────────────────────────────

/// undo/redo の適用が完了したフレームに発火するイベント。
/// `sync_strategy_buffer_to_editor_system` がこのイベントを受けて
/// エディタ表示を更新する（通常のユーザー入力では発火しない）。
#[derive(Message, Debug, Clone)]
pub struct UndoRedoApplied;

/// Strategy Editor の内容を復元するためのキュー。
/// `apply_pending_app_edits_system` が SpawnWindow を処理したフレームに積み、
/// `apply_strategy_snapshot_restore_system` が翌フレーム以降に処理する。
#[derive(Resource, Default, Debug)]
pub struct PendingStrategySnapshotRestore {
    pub snapshot: Option<(String, String)>,
}

// ─────────────────────────────────────────────────────────────────────────────
// ActiveDrag Resource — DragStart 時の位置を保存
// ─────────────────────────────────────────────────────────────────────────────

/// DragStart 時の元位置を保存するマップ。
/// DragEnd 時に before/after を比較して WindowMove を push する。
#[derive(Resource, Default, Debug)]
pub struct ActiveDrag {
    pub starts: std::collections::HashMap<Entity, Vec2>,
}

// ─────────────────────────────────────────────────────────────────────────────
// テスト
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_text_edit(before: &str, after: &str) -> AppEdit {
        AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: before.to_string(),
            after: after.to_string(),
            timestamp: Instant::now(),
        })
    }

    // ── edit / undo が pending に正しく push されるか ────────────────────

    #[test]
    fn test_text_edit_pushes_after_on_edit() {
        let mut pending = PendingAppEdits::default();
        let mut edit = make_text_edit("before", "after");
        edit.edit(&mut pending);
        assert_eq!(pending.queue.len(), 1);
        match &pending.queue[0] {
            AppEditAction::SetStrategySource { text, .. } => assert_eq!(text, "after"),
            _ => panic!("expected SetStrategySource"),
        }
    }

    #[test]
    fn test_text_edit_pushes_before_on_undo() {
        let mut pending = PendingAppEdits::default();
        let mut edit = make_text_edit("before", "after");
        edit.undo(&mut pending);
        assert_eq!(pending.queue.len(), 1);
        match &pending.queue[0] {
            AppEditAction::SetStrategySource { text, .. } => assert_eq!(text, "before"),
            _ => panic!("expected SetStrategySource"),
        }
    }

    // ── Record を使った undo/redo 往復 ───────────────────────────────────
    //
    // マージポリシーでは 500ms 超過の場合にマージが抑止される。
    // sleep で確実に 2 エントリを作り、undo/redo 往復を検証する。

    #[test]
    fn test_record_undo_redo_text() {
        use std::time::Duration;

        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        // 1 エントリ目を push
        record.edit(&mut pending, make_text_edit("", "hello"));
        pending.queue.clear(); // 通常入力時は clear する設計

        // 600ms 待機して確実に 500ms タイムウィンドウを超過させる → マージ不可
        std::thread::sleep(Duration::from_millis(600));

        record.edit(&mut pending, make_text_edit("hello", "hello world"));
        pending.queue.clear();

        assert_eq!(
            record.len(),
            2,
            "500ms 超過によりマージが抑止されて 2 エントリになるはず"
        );

        // undo → "hello" が pending に積まれる
        record.undo(&mut pending);
        assert_eq!(pending.queue.len(), 1);
        match &pending.queue[0] {
            AppEditAction::SetStrategySource { text, .. } => assert_eq!(text, "hello"),
            _ => panic!("expected SetStrategySource(hello)"),
        }
        pending.queue.clear();

        // redo → "hello world" が pending に積まれる
        record.redo(&mut pending);
        assert_eq!(pending.queue.len(), 1);
        match &pending.queue[0] {
            AppEditAction::SetStrategySource { text, .. } => assert_eq!(text, "hello world"),
            _ => panic!("expected SetStrategySource(hello world)"),
        }
    }

    #[test]
    fn test_record_single_entry_undo() {
        // マージされた場合: 1 エントリで undo すると最初の before に戻る
        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        // 両方とも Instant::now() でマージ対象 & 末尾は 'l'（alphanumeric） → マージされる
        record.edit(&mut pending, make_text_edit("", "hel"));
        pending.queue.clear();
        record.edit(&mut pending, make_text_edit("hel", "hello"));
        pending.queue.clear();

        // マージが起きていれば 1 エントリ、起きていなければ 2 エントリ
        // いずれにせよ undo して before への到達を検証する
        record.undo(&mut pending);
        // 最初の undo で before に戻ることだけ確認（マージ有無で before が違う）
        assert_eq!(pending.queue.len(), 1);
        match &pending.queue[0] {
            AppEditAction::SetStrategySource { .. } => {} // OK: SetStrategySource が来た
            _ => panic!("expected SetStrategySource"),
        }
    }

    // ── AppHistory::is_replaying ─────────────────────────────────────────

    #[test]
    fn test_is_replaying_depth() {
        let mut history = AppHistory::default();
        assert!(!history.is_replaying());

        history.replaying_depth += 1;
        assert!(history.is_replaying());

        history.replaying_depth -= 1;
        assert!(!history.is_replaying());
    }

    // ── push_text が is_replaying 中は抑止される ────────────────────────

    #[test]
    fn test_push_text_suppressed_while_replaying() {
        let mut history = AppHistory::default();
        history.replaying_depth = 1;
        history.push_text(
            "region_001".to_string(),
            "before".to_string(),
            "after".to_string(),
        );
        assert_eq!(history.record.len(), 0);
    }

    // ── テキストマージポリシー ───────────────────────────────────────────

    #[test]
    fn test_text_merge_within_window() {
        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        // 同一 Instant で作成（< 500ms は確実）
        let edit1 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "".to_string(),
            after: "hel".to_string(),
            timestamp: Instant::now(),
        });
        // 少し後（数μs 以内）で 差分 2 文字・改行なし・末尾 l（英字）= マージ
        let edit2 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "hel".to_string(),
            after: "hello".to_string(),
            timestamp: Instant::now(),
        });

        record.edit(&mut pending, edit1);
        pending.queue.clear();
        record.edit(&mut pending, edit2);
        pending.queue.clear();

        // マージされて 1 エントリになっているはず
        assert_eq!(record.len(), 1, "2 つの TextEdit がマージされるべき");
    }

    #[test]
    fn test_text_no_merge_different_types() {
        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        let text_edit = make_text_edit("a", "ab");
        let move_edit = AppEdit::WindowMove(WindowMoveEdit {
            kind: PanelKind::Orders,
            region_key: None,
            before: Vec2::ZERO,
            after: Vec2::new(100.0, 0.0),
        });

        record.edit(&mut pending, text_edit);
        pending.queue.clear();
        record.edit(&mut pending, move_edit);
        pending.queue.clear();

        // 型が違うのでマージされずに 2 エントリ
        assert_eq!(record.len(), 2, "型が違う場合はマージされない");
    }

    // ── WindowMove の push/undo ──────────────────────────────────────────

    #[test]
    fn test_window_move_undo() {
        let mut pending = PendingAppEdits::default();
        let mut edit = AppEdit::WindowMove(WindowMoveEdit {
            kind: PanelKind::Orders,
            region_key: None,
            before: Vec2::new(10.0, 20.0),
            after: Vec2::new(100.0, 200.0),
        });

        edit.edit(&mut pending);
        match &pending.queue[0] {
            AppEditAction::MoveWindow {
                kind: k,
                position: p,
                ..
            } => {
                assert_eq!(*k, PanelKind::Orders);
                assert_eq!(*p, Vec2::new(100.0, 200.0));
            }
            _ => panic!("expected MoveWindow(after)"),
        }
        pending.queue.clear();

        edit.undo(&mut pending);
        match &pending.queue[0] {
            AppEditAction::MoveWindow {
                kind: k,
                position: p,
                ..
            } => {
                assert_eq!(*k, PanelKind::Orders);
                assert_eq!(*p, Vec2::new(10.0, 20.0));
            }
            _ => panic!("expected MoveWindow(before)"),
        }
    }

    // ── WindowSpawn undo は DespawnWindow { kind } を push ───────────────

    fn dummy_layout(kind: PanelKind) -> crate::ui::layout_persistence::WindowLayout {
        crate::ui::layout_persistence::WindowLayout {
            kind,
            visible: true,
            position: [0.0, 0.0],
            size: [100.0, 100.0],
            z: 10.0,
            region_key: None,
        }
    }

    #[test]
    fn test_window_spawn_undo_despawns() {
        let mut pending = PendingAppEdits::default();
        let mut edit = AppEdit::WindowSpawn(WindowSpawnEdit {
            kind: PanelKind::Orders,
            layout: dummy_layout(PanelKind::Orders),
        });

        edit.undo(&mut pending);
        assert_eq!(pending.queue.len(), 1);
        match &pending.queue[0] {
            AppEditAction::DespawnWindow { kind, .. } => assert_eq!(*kind, PanelKind::Orders),
            _ => panic!("expected DespawnWindow"),
        }
    }

    // ── WindowDespawn undo は SpawnWindow を push ────────────────────────

    #[test]
    fn test_window_despawn_undo_spawns() {
        let mut pending = PendingAppEdits::default();
        let mut edit = AppEdit::WindowDespawn(WindowDespawnEdit {
            layout: dummy_layout(PanelKind::StrategyEditor),
            strategy_snapshot: None,
        });

        edit.undo(&mut pending);
        assert_eq!(pending.queue.len(), 1);
        match &pending.queue[0] {
            AppEditAction::SpawnWindow {
                layout,
                strategy_snapshot,
            } => {
                assert_eq!(layout.kind, PanelKind::StrategyEditor);
                assert!(strategy_snapshot.is_none());
            }
            _ => panic!("expected SpawnWindow(StrategyEditor)"),
        }
    }

    // ── マージポリシー緩和: スペース・記号でもマージされる ─────────────

    #[test]
    fn test_merge_space_input() {
        // スペース入力でもマージされることを確認（last_char_is_word_boundary 条件廃止後の挙動）
        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        let edit1 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "".to_string(),
            after: "hello".to_string(),
            timestamp: Instant::now(),
        });
        let edit2 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "hello".to_string(),
            after: "hello ".to_string(), // 末尾にスペース追加
            timestamp: Instant::now(),
        });

        record.edit(&mut pending, edit1);
        pending.queue.clear();
        record.edit(&mut pending, edit2);
        pending.queue.clear();

        assert_eq!(record.len(), 1, "スペース入力でもマージされるべき");
    }

    #[test]
    fn test_merge_symbol_inputs() {
        // ':', '(', ')' などの記号入力でもマージされることを確認
        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        // ':' を追加
        let e1 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "".to_string(),
            after: "foo".to_string(),
            timestamp: Instant::now(),
        });
        let e2 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "foo".to_string(),
            after: "foo:".to_string(),
            timestamp: Instant::now(),
        });
        record.edit(&mut pending, e1);
        pending.queue.clear();
        record.edit(&mut pending, e2);
        pending.queue.clear();
        assert_eq!(record.len(), 1, "':' 入力でもマージされるべき");

        let mut record2: Record<AppEdit> = Record::new();
        // '(' を追加
        let e3 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "".to_string(),
            after: "fn".to_string(),
            timestamp: Instant::now(),
        });
        let e4 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "fn".to_string(),
            after: "fn(".to_string(),
            timestamp: Instant::now(),
        });
        record2.edit(&mut pending, e3);
        pending.queue.clear();
        record2.edit(&mut pending, e4);
        pending.queue.clear();
        assert_eq!(record2.len(), 1, "'(' 入力でもマージされるべき");
    }

    #[test]
    fn test_no_merge_newline() {
        // 改行入力はマージされない
        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        let edit1 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "".to_string(),
            after: "hello".to_string(),
            timestamp: Instant::now(),
        });
        let edit2 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "hello".to_string(),
            after: "hello\nworld".to_string(), // 改行追加
            timestamp: Instant::now(),
        });

        record.edit(&mut pending, edit1);
        pending.queue.clear();
        record.edit(&mut pending, edit2);
        pending.queue.clear();

        assert_eq!(record.len(), 2, "改行入力はマージされない");
    }

    #[test]
    fn test_no_merge_large_paste() {
        // 差分が TEXT_MERGE_MAX_DIFF (50) を超える大きな paste はマージされない
        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        let large_text = "a".repeat(100); // 100 文字の paste
        let edit1 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "".to_string(),
            after: "start".to_string(),
            timestamp: Instant::now(),
        });
        let edit2 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "start".to_string(),
            after: format!("start{}", large_text), // 差分 100 文字
            timestamp: Instant::now(),
        });

        record.edit(&mut pending, edit1);
        pending.queue.clear();
        record.edit(&mut pending, edit2);
        pending.queue.clear();

        assert_eq!(
            record.len(),
            2,
            "大きな paste (差分 100 文字) はマージされない"
        );
    }

    #[test]
    fn test_no_merge_timeout() {
        // 500ms 超過はマージされない
        use std::time::Duration;

        let mut record: Record<AppEdit> = Record::new();
        let mut pending = PendingAppEdits::default();

        let edit1 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "".to_string(),
            after: "hello".to_string(),
            timestamp: Instant::now(),
        });
        record.edit(&mut pending, edit1);
        pending.queue.clear();

        std::thread::sleep(Duration::from_millis(600));

        let edit2 = AppEdit::Text(TextEdit {
            region_key: "region_001".to_string(),
            before: "hello".to_string(),
            after: "hello world".to_string(),
            timestamp: Instant::now(),
        });
        record.edit(&mut pending, edit2);
        pending.queue.clear();

        assert_eq!(record.len(), 2, "500ms 超過はマージされない");
    }
}
