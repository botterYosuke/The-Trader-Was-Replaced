//! 構造化 UI ダンプ — 目視確認の代わりに「画面を構成する要素」を
//! 位置 / 大きさ / 表示・非表示 / キャプション のパラメータとして取り出し、
//! headless テストで assert するための補助。
//!
//! 対象は world-space の floating panel（`WindowRoot` + `Sprite` + `Transform`）。
//! これらは spawn 時に Transform（位置・z）と Sprite.custom_size（大きさ）と
//! Visibility（表示）が確定するため、render プラグイン無しの bare `App` でも忠実に読める。
//! 配下の `Text2d` を全収集してキャプション（タイトル・値ラベル）とする。
//!
//! 注意: `Node`+flexbox の操作系 UI（footer/sidebar/menu/order_panel 等）の
//! **位置・大きさは computed layout（`ComputedNode`）依存で headless では確定しない**。
//! それらの表示/非表示だけを見たい場合は [`marker_display`] を使う（`Node.display` は確定する）。

// 補助 API は flow ごとに使う関数/フィールドが異なり、未使用分が出るのは想定内。
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use backcast::ui::components::{ChartInstrument, PanelKind, StrategyEditorId, WindowRoot};

/// 1 つの world-space パネルのスナップショット。
#[derive(Debug, Clone)]
pub struct PanelSnapshot {
    /// `PanelKind::label`（"Strategy Editor" / "Chart" など）。
    pub kind: &'static str,
    /// パネル中心の world 座標（`Transform.translation` の x/y）。
    pub position: Vec2,
    /// 重なり順（`Transform.translation.z`）。
    pub z: f32,
    /// パネルの大きさ（`Sprite.custom_size`。無ければ `Vec2::ZERO`）。
    pub size: Vec2,
    /// `Visibility != Hidden`。apply_layout が visible=false の window に立てる Hidden を反映。
    pub visible: bool,
    /// 配下（自分含む子孫）の `Text2d` をすべて収集したもの。タイトルや値ラベルが入る。
    pub captions: Vec<String>,
    /// Strategy Editor のときの region_key。
    pub region_key: Option<String>,
    /// Chart のときの instrument_id。
    pub instrument_id: Option<String>,
}

impl PanelSnapshot {
    /// captions のいずれかが `substr` を含むか。
    pub fn has_caption(&self, substr: &str) -> bool {
        self.captions.iter().any(|c| c.contains(substr))
    }

    /// `has_caption` の大文字小文字無視版（タイトルバーは大文字化されるため）。
    pub fn has_caption_ci(&self, substr: &str) -> bool {
        let needle = substr.to_lowercase();
        self.captions
            .iter()
            .any(|c| c.to_lowercase().contains(&needle))
    }
}

/// `Node` の width/height(Px) を `Vec2` にする（screen-space window のサイズ読み取り用）。
fn node_px_size(node: &Node) -> Vec2 {
    let px = |v: Val| if let Val::Px(p) = v { p } else { 0.0 };
    Vec2::new(px(node.width), px(node.height))
}

/// すべての `WindowRoot` パネルを位置 / 大きさ / 表示 / キャプション付きでダンプする。
pub fn dump_panels(world: &mut World) -> Vec<PanelSnapshot> {
    // children / Text2d を 1 度ずつ収集してマップ化（root ごとのネスト query を避ける）。
    let mut children_map: HashMap<Entity, Vec<Entity>> = HashMap::new();
    {
        let mut q = world.query::<(Entity, &Children)>();
        for (e, ch) in q.iter(world) {
            children_map.insert(e, ch.iter().collect());
        }
    }
    // world-space は `Text2d`、screen-space（ADR 0003 の draggable window）は UI `Text`。
    // 両方を集めてキャプション化する。
    let mut text_map: HashMap<Entity, String> = HashMap::new();
    {
        let mut q = world.query::<(Entity, &Text2d)>();
        for (e, t) in q.iter(world) {
            text_map.insert(e, t.0.clone());
        }
    }
    {
        let mut q = world.query::<(Entity, &Text)>();
        for (e, t) in q.iter(world) {
            text_map.entry(e).or_insert_with(|| t.0.clone());
        }
    }

    let mut out: Vec<(Entity, PanelSnapshot)> = Vec::new();
    {
        let mut q = world.query_filtered::<
            (
                Entity,
                &PanelKind,
                &Transform,
                Option<&Sprite>,
                Option<&Node>,
                &Visibility,
                Option<&StrategyEditorId>,
                Option<&ChartInstrument>,
            ),
            With<WindowRoot>,
        >();
        for (e, kind, tf, sprite, node, vis, sid, chart) in q.iter(world) {
            // world-space window は Sprite.custom_size、screen-space window は Node の width/height(Px)。
            let size = sprite
                .and_then(|s| s.custom_size)
                .or_else(|| node.map(node_px_size))
                .unwrap_or(Vec2::ZERO);
            out.push((
                e,
                PanelSnapshot {
                    kind: kind.label(),
                    position: tf.translation.truncate(),
                    z: tf.translation.z,
                    size,
                    visible: !matches!(vis, Visibility::Hidden),
                    captions: Vec::new(),
                    region_key: sid.map(|s| s.region_key.clone()),
                    instrument_id: chart.map(|c| c.instrument_id.clone()),
                },
            ));
        }
    }

    // 各 root 配下の Text2d を BFS で収集してキャプションにする。
    for (root, snap) in out.iter_mut() {
        let mut stack = vec![*root];
        let mut seen = HashSet::new();
        while let Some(e) = stack.pop() {
            if !seen.insert(e) {
                continue;
            }
            if let Some(t) = text_map.get(&e) {
                snap.captions.push(t.clone());
            }
            if let Some(ch) = children_map.get(&e) {
                stack.extend(ch.iter().copied());
            }
        }
    }

    out.into_iter().map(|(_, s)| s).collect()
}

/// `kind` ラベル（`PanelKind::label`）が一致するパネルだけ返す。
pub fn panels_of<'a>(snaps: &'a [PanelSnapshot], kind: &str) -> Vec<&'a PanelSnapshot> {
    snaps.iter().filter(|s| s.kind == kind).collect()
}

/// 描画要素の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Sprite,
    Text,
}

/// 画面を構成する個々の描画要素（`Sprite` または `Text2d`）1 つ分のスナップショット。
/// パネルの子要素（タイトル・値ラベル・gutter・ボタン背景など）も粒度で拾う。
#[derive(Debug, Clone)]
pub struct ElementSnapshot {
    pub kind: ElementKind,
    /// `GlobalTransform` の絶対 world 座標（x/y）。子要素も親からの伝播後の値。
    pub position: Vec2,
    pub z: f32,
    /// `Sprite.custom_size`（`Text2d` は `None`）。
    pub size: Option<Vec2>,
    pub visible: bool,
    /// `Text2d` のテキスト（`Sprite` は `None`）。
    pub caption: Option<String>,
}

/// 画面を構成する全描画要素（`Sprite` / `Text2d` を持つ entity）を絶対位置付きでダンプする。
/// `GlobalTransform` を読むため、呼び出し側 `App` に `TransformPlugin` を入れ、
/// `app.update()`（伝播は PostUpdate）後に呼ぶこと。
pub fn dump_elements(world: &mut World) -> Vec<ElementSnapshot> {
    let mut out = Vec::new();
    let mut q = world.query_filtered::<
        (&GlobalTransform, &Visibility, Option<&Sprite>, Option<&Text2d>),
        Or<(With<Sprite>, With<Text2d>)>,
    >();
    for (gt, vis, sprite, text) in q.iter(world) {
        let p = gt.translation();
        out.push(ElementSnapshot {
            kind: if sprite.is_some() {
                ElementKind::Sprite
            } else {
                ElementKind::Text
            },
            position: p.truncate(),
            z: p.z,
            size: sprite.and_then(|s| s.custom_size),
            visible: !matches!(vis, Visibility::Hidden),
            caption: text.map(|t| t.0.clone()),
        });
    }
    out
}

/// marker `M` を持つ `Node` 要素の表示状態を返す（操作系 UI 用）。
/// `Node.display != Display::None` を表示とみなす。位置・大きさは computed layout
/// 依存で headless では取れないため、ここでは表示/非表示のみを扱う。
pub fn marker_display<M: Component>(world: &mut World) -> Vec<bool> {
    let mut q = world.query_filtered::<&Node, With<M>>();
    q.iter(world)
        .map(|node| node.display != Display::None)
        .collect()
}
