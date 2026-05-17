//! Phase 7.5b — Instrument Picker module。
//!
//! 計画書 `docs/plan/Phase 7.5b - Instrument Picker.md` §2 / §3 に対応する
//! scaffolding。Resource / Component / spawn helper の宣言のみ。
//! system 実装はサブ C 以降で追加する。
//!
//! 重要:
//! - picker root には必ず `LayoutExcluded` を同時に付与すること（§3.6 / R10）。
//! - `InstrumentPickerState` は `UiPlugin::build` で `init_resource` される。

use bevy::prelude::*;
use chrono::NaiveDate;
use std::time::Instant;

use crate::ui::components::LayoutExcluded;

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

#[derive(Resource, Debug, Default, Clone)]
pub struct InstrumentPickerState {
    pub visible: bool,
    pub end_date: Option<NaiveDate>,
    pub query: String,
    pub last_opened_at: Option<Instant>,
    pub last_added: Option<String>,
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

#[derive(Component, Debug, Default, Clone, Copy)]
pub struct InstrumentPickerWindow;

#[derive(Component, Debug, Default, Clone, Copy)]
pub struct InstrumentPickerSearchBox;

#[derive(Component, Debug, Default, Clone, Copy)]
pub struct InstrumentPickerListContainer;

#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerRow {
    pub instrument_id: String,
    pub already_added: bool,
}

#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerAddButton {
    pub instrument_id: String,
}

// ---------------------------------------------------------------------------
// Spawn helper
// ---------------------------------------------------------------------------

pub fn spawn_picker_root(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            InstrumentPickerWindow,
            LayoutExcluded,
            Transform::default(),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            Name::new("InstrumentPickerWindow"),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Systems (stub — サブ C 以降で実装)
// ---------------------------------------------------------------------------
//
// TODO(Phase 7.5b サブ C): open/close toggle system
// TODO(Phase 7.5b サブ C): search query update system
// TODO(Phase 7.5b サブ D): list rebuild system
// TODO(Phase 7.5b サブ D): row Add button click system
// TODO(Phase 7.5b サブ E): UiPlugin への add_systems 配線
