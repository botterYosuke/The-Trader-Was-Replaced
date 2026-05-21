//! Phase 9 §3.10 — SecretRequired モーダル (Tachibana 第二暗証番号入力、Tachibana 専用)。
//!
//! `SubscribeBackendEvents` の `SecretRequired` を `backend_event_drain_system` (main.rs) が
//! `SecretPrompt.active` にセット → 本モーダルが開く。kabu / mock は Password 不要なので
//! `SecretRequired` を発行せず、このモーダルは開かない。
//!
//! ユーザー選択 (2026-05-20) で UI は Bevy UI Node 流派。入力は cosmic_edit ではなく
//! **keyboard イベント drain → `Zeroizing<String>` バッファ** で受ける (picker_searchbox_input_system
//! と同じ drain パターン)。これにより (a) 平文を resource に滞留させない (`Zeroizing` が
//! ドロップ時に memory を 0 埋め)、(b) cosmic_edit の内部 buffer を zeroize する困難
//! (DPI/attrs の罠も含む) を回避し、(c) ログ・状態に平文が出ない (`RedactedSecret` が Debug を
//! 伏字化) という §1.3 / §6 のセキュリティ目標を素直に満たす。
//!
//! **計画書ドリフト訂正**: §3.10 は「cosmic-edit 1 行 password モード」と書くが、
//! egui 撤去後の本コードベースに UI-node 向け cosmic password フィールドの実績がなく、
//! buffer zeroize も困難。実際の AC (§6) は「明示保持しない / zeroize / 平文を残さない」で
//! あり、keyboard-drain + `Zeroizing` の方が確実に満たすため流派を変更する。

use std::time::{Duration, Instant};

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use zeroize::Zeroizing;

use crate::trading::{
    OrderFeedback, RedactedSecret, SecretPrompt, TransportCommand, TransportCommandSender,
};

/// バックエンドの 30s タイムアウトより少し短く設定し、先に UI を畳む (§3.10)。
const SECRET_INPUT_TIMEOUT: Duration = Duration::from_secs(25);

const COLOR_PANEL_BG: Color = Color::srgba(0.07, 0.07, 0.12, 0.98);
const COLOR_BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const COLOR_HEADER: Color = Color::srgb(0.0, 0.81, 1.0);
const COLOR_INFO: Color = Color::srgb(0.65, 0.70, 0.78);
const COLOR_VALUE: Color = Color::srgb(0.88, 0.91, 0.96);
const COLOR_BTN_SUBMIT: Color = Color::srgba(0.10, 0.45, 0.30, 1.0);
const COLOR_BTN_CANCEL: Color = Color::srgba(0.30, 0.16, 0.20, 1.0);
const COLOR_FIELD_BG: Color = Color::srgba(0.04, 0.04, 0.08, 1.0);

// ===========================================================================
// Resource — 平文バッファ (Zeroizing) + 開始時刻 (timeout 用)
// ===========================================================================

/// 入力中の第二暗証番号を保持する。`Zeroizing<String>` はドロップ・置換時に
/// backing memory を 0 埋めする。平文はここ以外 (ログ・他 resource・ファイル) には出さない。
/// `Debug` は **意図的に derive しない** (平文の漏洩防止)。
#[derive(Resource, Default)]
pub struct SecretInput {
    buffer: Zeroizing<String>,
    opened_at: Option<Instant>,
    /// The `request_id` the current buffer belongs to. Lets the lifecycle system
    /// detect a supersede (a new `SecretRequired` with a different id replacing an
    /// in-flight prompt) and zeroize the carried-over plaintext (Round 1 bevy M1).
    request_id: Option<String>,
}

impl SecretInput {
    pub fn len(&self) -> usize {
        self.buffer.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    fn push_char(&mut self, c: char) {
        if !c.is_control() {
            self.buffer.push(c);
        }
    }

    fn backspace(&mut self) {
        self.buffer.pop();
    }

    /// バッファ・開始時刻・request_id を破棄する。古い `Zeroizing` は置換ドロップで 0 埋め。
    fn clear(&mut self) {
        self.buffer = Zeroizing::new(String::new());
        self.opened_at = None;
        self.request_id = None;
    }
}

// ===========================================================================
// Components
// ===========================================================================

#[derive(Component)]
pub struct SecretModalRoot;

/// venue / purpose を出す情報行。
#[derive(Component)]
pub struct SecretInfoText;

/// マスク表示 (•) のテキスト。
#[derive(Component)]
pub struct SecretMaskedText;

#[derive(Component, Clone, Copy)]
pub enum SecretButton {
    Submit,
    Cancel,
}

// ===========================================================================
// 共有アクション (Enter/Submit ボタン と Esc/Cancel ボタンで共用)
// ===========================================================================

/// 入力済み secret を `SubmitSecret` で送り、prompt を閉じ、バッファを zeroize する。
fn do_submit(
    input: &mut SecretInput,
    prompt: &mut SecretPrompt,
    sender: Option<&TransportCommandSender>,
) {
    // 空送信は無視し prompt を開いたままにする。空 secret を送ると一回限りの
    // request_id を浪費し、Tachibana の失敗回数制限を空打ちで削る (§9 Open Risk 1)。
    if input.is_empty() {
        return;
    }
    let Some(req) = prompt.active.take() else {
        return;
    };
    // Clear any prior submit error before resubmitting (§3.10).
    prompt.error = None;
    // to_string() は平文を新 String にコピーするが、即 RedactedSecret(Zeroizing) に
    // move されるため滞留しない。送信後はコマンドごとドロップされる (§1.3)。
    let secret = RedactedSecret::new(input.buffer.to_string());
    match sender {
        Some(tx) => {
            let _ = tx.tx.send(TransportCommand::SubmitSecret {
                request_id: req.request_id,
                secret,
            });
        }
        None => warn!("SubmitSecret skipped: TransportCommandSender unavailable"),
    }
    input.clear();
}

/// prompt を閉じてバッファを zeroize する (発注はしない)。
fn do_cancel(input: &mut SecretInput, prompt: &mut SecretPrompt, reason: &str) {
    if prompt.active.is_some() {
        // 平文は出さない。理由コードのみ。
        warn!("[secret] modal closed: {reason}");
    }
    // close() drops both `active` and any stale submit error (§3.10).
    prompt.close();
    input.clear();
}

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

pub fn spawn_secret_modal(mut commands: Commands) {
    commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(COLOR_BACKDROP),
            // 確認モーダル (200) より前面に。secret 収集は最優先。
            GlobalZIndex(300),
            SecretModalRoot,
            Name::new("SecretModal"),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Px(320.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
            ))
            .with_children(|card| {
                card.spawn((
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                    Text::new("第二暗証番号を入力"),
                    TextFont {
                        font_size: 15.0,
                        ..default()
                    },
                    TextColor(COLOR_HEADER),
                ));
                card.spawn((
                    Node {
                        // Bound the width to the card's content box so a long
                        // SubmitSecret error line (§3.10, appended after a `\n`)
                        // wraps inside the 320px card instead of overflowing.
                        width: Val::Percent(100.0),
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(COLOR_INFO),
                    SecretInfoText,
                ));
                // マスクフィールド
                card.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(30.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(COLOR_FIELD_BG),
                ))
                .with_children(|field| {
                    field.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(COLOR_VALUE),
                        SecretMaskedText,
                    ));
                });
                // ボタン行
                card.spawn((Node {
                    margin: UiRect::top(Val::Px(14.0)),
                    column_gap: Val::Px(10.0),
                    ..default()
                },))
                    .with_children(|btns| {
                        btns.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(30.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(COLOR_BTN_CANCEL),
                            SecretButton::Cancel,
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new("キャンセル"),
                                TextFont {
                                    font_size: 13.0,
                                    ..default()
                                },
                                TextColor(COLOR_VALUE),
                            ));
                        });
                        btns.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(30.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(COLOR_BTN_SUBMIT),
                            SecretButton::Submit,
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new("送信"),
                                TextFont {
                                    font_size: 13.0,
                                    ..default()
                                },
                                TextColor(COLOR_VALUE),
                            ));
                        });
                    });
            });
        });
}

// ===========================================================================
// Systems
// ===========================================================================

/// prompt の open/close/supersede に追従してバッファを管理する。
/// 開いた瞬間、または **別 request_id の SecretRequired が現在の prompt を置き換えた**瞬間に
/// 古い入力を zeroize し `opened_at` を打ち直す (timeout 起点)。同一 request_id の間は
/// バッファ・計時を保持する (ユーザーが入力中)。`(true,true)` で request_id が変わった場合に
/// 旧 request の平文が新 prompt へ持ち越されるのを防ぐ (Round 1 bevy M1、§6 平文を残さない)。
pub fn secret_modal_lifecycle_system(prompt: Res<SecretPrompt>, mut input: ResMut<SecretInput>) {
    match prompt.active.as_ref() {
        Some(req) => {
            let same_request = input.request_id.as_deref() == Some(req.request_id.as_str());
            if !same_request {
                input.clear(); // open または supersede: 旧バッファを 0 埋め
                input.request_id = Some(req.request_id.clone());
                input.opened_at = Some(Instant::now());
            }
        }
        None => {
            // 外部 (submit/cancel/timeout) で閉じられた残骸を掃除。
            if input.opened_at.is_some() || input.request_id.is_some() {
                input.clear();
            }
        }
    }
}

/// モーダル root の Display を `SecretPrompt.active` に同期する。
pub fn secret_modal_visibility_system(
    prompt: Res<SecretPrompt>,
    mut root_q: Query<&mut Node, With<SecretModalRoot>>,
) {
    let target = if prompt.active.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut root_q {
        if node.display != target {
            node.display = target;
        }
    }
}

/// モーダル表示中だけ keyboard を drain してバッファに反映する。
/// drain することで cosmic_edit / menu への二重配送を防ぐ (picker と同じ手法)。
/// Enter = 送信、Esc = キャンセル。
pub fn secret_modal_input_system(
    mut prompt: ResMut<SecretPrompt>,
    mut input: ResMut<SecretInput>,
    mut kb_events: ResMut<Events<KeyboardInput>>,
    sender: Option<Res<TransportCommandSender>>,
) {
    if prompt.active.is_none() {
        return;
    }
    let mut submit = false;
    let mut cancel = false;
    for ev in kb_events.drain() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Character(s) => {
                for ch in s.chars() {
                    input.push_char(ch);
                }
            }
            Key::Space => input.push_char(' '),
            Key::Backspace => input.backspace(),
            Key::Enter => submit = true,
            Key::Escape => cancel = true,
            _ => {}
        }
    }
    // drain 後に判定 (同フレームに複数キーが来ても最後の意図を優先しない単純化:
    // Esc が来ていれば cancel を優先する)。
    if cancel {
        do_cancel(&mut input, &mut prompt, "SECRET_INPUT_CANCELED (escape)");
    } else if submit {
        do_submit(&mut input, &mut prompt, sender.as_deref());
    }
}

/// 送信 / キャンセルボタンを処理する。
pub fn secret_modal_button_system(
    interactions: Query<(&Interaction, &SecretButton), (Changed<Interaction>, With<Button>)>,
    mut prompt: ResMut<SecretPrompt>,
    mut input: ResMut<SecretInput>,
    sender: Option<Res<TransportCommandSender>>,
) {
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            SecretButton::Submit => do_submit(&mut input, &mut prompt, sender.as_deref()),
            SecretButton::Cancel => {
                do_cancel(&mut input, &mut prompt, "SECRET_INPUT_CANCELED (button)")
            }
        }
    }
}

/// 25s でモーダルを auto-close する (§3.10)。タイムアウト時はユーザーに
/// `SECRET_INPUT_CANCELED` を OrderPanel エラー行で通知する (toast 基盤が無いため)。
pub fn secret_modal_timeout_system(
    mut prompt: ResMut<SecretPrompt>,
    mut input: ResMut<SecretInput>,
    mut feedback: ResMut<OrderFeedback>,
) {
    if prompt.active.is_none() {
        return;
    }
    let Some(opened) = input.opened_at else {
        return;
    };
    if opened.elapsed() >= SECRET_INPUT_TIMEOUT {
        do_cancel(&mut input, &mut prompt, "SECRET_INPUT_CANCELED (timeout)");
        feedback.message =
            Some("第二暗証番号の入力がタイムアウトしました (SECRET_INPUT_CANCELED)".to_string());
    }
}

/// マスク (•) と venue/purpose 情報を差分反映する。
pub fn secret_modal_sync_system(
    prompt: Res<SecretPrompt>,
    input: Res<SecretInput>,
    mut masked_q: Query<&mut Text, (With<SecretMaskedText>, Without<SecretInfoText>)>,
    mut info_q: Query<&mut Text, (With<SecretInfoText>, Without<SecretMaskedText>)>,
) {
    let mask: String = "•".repeat(input.len());
    if let Ok(mut t) = masked_q.get_single_mut()
        && t.0 != mask
    {
        t.0 = mask;
    }
    let info = match prompt.active.as_ref() {
        Some(req) => {
            let base = format!("venue: {} / purpose: {}", req.venue, req.purpose);
            // §3.10: a failed SubmitSecret surfaces here (NOT the OrderPanel) so the
            // user can retry within this same modal.
            match prompt.error.as_ref() {
                Some(err) => format!("{base}\n{err}"),
                None => base,
            }
        }
        None => String::new(),
    };
    if let Ok(mut t) = info_q.get_single_mut()
        && t.0 != info
    {
        t.0 = info;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::SecretPromptRequest;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<SecretInput>();
        app.init_resource::<SecretPrompt>();
        app.init_resource::<OrderFeedback>();
        app
    }

    fn activate(app: &mut App, request_id: &str) {
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: request_id.to_string(),
            venue: "tachibana".to_string(),
            kind: "second_secret".to_string(),
            purpose: "new_order".to_string(),
        });
    }

    #[test]
    fn lifecycle_arms_timer_and_clears_on_open() {
        let mut app = make_app();
        // 残骸を入れておく
        app.world_mut().resource_mut::<SecretInput>().push_char('x');
        activate(&mut app, "r1");
        app.add_systems(Update, secret_modal_lifecycle_system);
        app.update();
        let input = app.world().resource::<SecretInput>();
        assert!(input.is_empty(), "open must zeroize stale buffer");
        assert!(input.opened_at.is_some(), "open must arm the timeout clock");
    }

    #[test]
    fn input_accumulates_and_masks_length() {
        let mut input = SecretInput::default();
        input.push_char('1');
        input.push_char('2');
        input.push_char('3');
        assert_eq!(input.len(), 3);
        input.backspace();
        assert_eq!(input.len(), 2);
        // control 文字は無視
        input.push_char('\n');
        assert_eq!(input.len(), 2);
    }

    #[test]
    fn submit_fires_submit_secret_and_zeroizes() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        activate(&mut app, "req-42");
        {
            let mut input = app.world_mut().resource_mut::<SecretInput>();
            input.push_char('p');
            input.push_char('w');
        }
        app.add_systems(Update, secret_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Submit));
        app.update();

        let cmd = rx.try_recv().expect("Submit must fire SubmitSecret");
        match cmd {
            TransportCommand::SubmitSecret { request_id, secret } => {
                assert_eq!(request_id, "req-42");
                assert_eq!(secret.expose(), "pw");
            }
            other => panic!("expected SubmitSecret, got {other:?}"),
        }
        let input = app.world().resource::<SecretInput>();
        assert!(input.is_empty(), "buffer must be zeroized after submit");
        assert!(
            app.world().resource::<SecretPrompt>().active.is_none(),
            "prompt must close after submit"
        );
    }

    #[test]
    fn cancel_closes_without_firing() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        activate(&mut app, "r1");
        app.world_mut().resource_mut::<SecretInput>().push_char('z');
        app.add_systems(Update, secret_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Cancel));
        app.update();

        assert!(rx.try_recv().is_err(), "Cancel must not fire a command");
        assert!(app.world().resource::<SecretPrompt>().active.is_none());
        assert!(app.world().resource::<SecretInput>().is_empty());
    }

    #[test]
    fn timeout_closes_modal() {
        let mut app = make_app();
        activate(&mut app, "r1");
        {
            let mut input = app.world_mut().resource_mut::<SecretInput>();
            input.push_char('a');
            // 26s 前に開いたことにする (timeout 超過)。
            input.opened_at = Some(Instant::now() - Duration::from_secs(26));
        }
        app.add_systems(Update, secret_modal_timeout_system);
        app.update();
        assert!(
            app.world().resource::<SecretPrompt>().active.is_none(),
            "expired prompt must auto-close"
        );
        assert!(app.world().resource::<SecretInput>().is_empty());
        assert!(
            app.world()
                .resource::<OrderFeedback>()
                .message
                .as_deref()
                .is_some_and(|m| m.contains("SECRET_INPUT_CANCELED")),
            "timeout must surface SECRET_INPUT_CANCELED to the user"
        );
    }

    #[test]
    fn lifecycle_zeroizes_on_supersede_with_different_request_id() {
        let mut app = make_app();
        app.add_systems(Update, secret_modal_lifecycle_system);
        // 1st request opens and the user types a partial PIN.
        activate(&mut app, "rA");
        app.update();
        app.world_mut().resource_mut::<SecretInput>().push_char('9');
        // A different SecretRequired supersedes before submit.
        activate(&mut app, "rB");
        app.update();
        let input = app.world().resource::<SecretInput>();
        assert!(
            input.is_empty(),
            "supersede by a different request_id must zeroize the carried-over PIN"
        );
    }

    #[test]
    fn empty_submit_is_noop_and_keeps_prompt_open() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        activate(&mut app, "r1");
        // no chars typed
        app.add_systems(Update, secret_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, SecretButton::Submit));
        app.update();
        assert!(
            rx.try_recv().is_err(),
            "empty buffer must not fire SubmitSecret (would waste the one-shot request_id)"
        );
        assert!(
            app.world().resource::<SecretPrompt>().active.is_some(),
            "prompt stays open so the user can still type"
        );
    }

    #[test]
    fn timeout_does_not_close_before_deadline() {
        let mut app = make_app();
        activate(&mut app, "r1");
        app.world_mut().resource_mut::<SecretInput>().opened_at = Some(Instant::now());
        app.add_systems(Update, secret_modal_timeout_system);
        app.update();
        assert!(
            app.world().resource::<SecretPrompt>().active.is_some(),
            "fresh prompt must stay open"
        );
    }
}
