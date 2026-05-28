//! LSP bridge: fan diagnostic markers (spawned by the LSP layer) out
//! into per-editor [`GlyphMarkers`] + [`GutterDecorations`]. Severity
//! → icon kind + colour via the editor's `DiagnosticColors`.

use bevy::prelude::*;
use lsp_types::DiagnosticSeverity;

use crate::types::CodeEditor;

use super::bars::{DecorationKind, GutterDecorations, LineDecoration};
use super::markers::{GlyphKind, GlyphMarker, GlyphMarkers};

pub(crate) fn sync_lsp_glyph_markers(
    diagnostics: Query<&crate::lsp_ui::systems::DiagnosticMarker>,
    mut editors: Query<
        (
            &crate::settings::DiagnosticColors,
            &mut GlyphMarkers,
            &mut GutterDecorations,
        ),
        With<CodeEditor>,
    >,
) {
    let mut per_line: std::collections::HashMap<usize, DiagnosticSeverity> = Default::default();
    for diag in diagnostics.iter() {
        let entry = per_line.entry(diag.line).or_insert(diag.severity);
        if severity_rank(diag.severity) > severity_rank(*entry) {
            *entry = diag.severity;
        }
    }
    for (colors, mut markers, mut decorations) in editors.iter_mut() {
        let mut new_markers: Vec<GlyphMarker> = Vec::with_capacity(per_line.len());
        let mut new_bars: Vec<LineDecoration> = Vec::with_capacity(per_line.len());
        for (&line, &severity) in &per_line {
            let (kind, color) = match severity {
                DiagnosticSeverity::ERROR => (GlyphKind::DiagnosticError, colors.error),
                DiagnosticSeverity::WARNING => (GlyphKind::DiagnosticWarning, colors.warning),
                DiagnosticSeverity::INFORMATION => (GlyphKind::DiagnosticInfo, colors.info),
                _ => (GlyphKind::DiagnosticHint, colors.hint),
            };
            new_markers.push(GlyphMarker { line, kind, color });
            new_bars.push(LineDecoration {
                line,
                kind: DecorationKind::DiagnosticBar,
                color,
            });
        }
        markers.0.retain(|m| {
            !matches!(
                m.kind,
                GlyphKind::DiagnosticError
                    | GlyphKind::DiagnosticWarning
                    | GlyphKind::DiagnosticInfo
                    | GlyphKind::DiagnosticHint
            )
        });
        decorations
            .0
            .retain(|d| !matches!(d.kind, DecorationKind::DiagnosticBar));
        markers.0.extend(new_markers);
        decorations.0.extend(new_bars);
    }
}

fn severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::ERROR => 4,
        DiagnosticSeverity::WARNING => 3,
        DiagnosticSeverity::INFORMATION => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    //! Headless tests for the LSP → gutter bridge. We drive the pipeline by
    //! writing an `LspDiagnosticsUpdated` event ourselves, then run two
    //! `Update` ticks: tick 1 lets `on_lsp_diagnostics` spawn the
    //! `DiagnosticMarker` entities, tick 2 lets `sync_lsp_glyph_markers` read
    //! them and populate the editor's `GlyphMarkers` / `GutterDecorations`.
    //!
    //! No rust-analyzer, no window, no asset pipeline — pure ECS.
    use super::*;
    use crate::lsp_ui::systems::{on_lsp_diagnostics, DiagnosticMarker};
    use crate::settings::{Misc, RenderSettings, RenderValidationDecorations};
    use bevy::app::{App, Update};
    use bevy::ecs::entity::Entity;
    use bevy_lsp::messages::LspDiagnosticsUpdated;
    use lsp_types::{Diagnostic, Position, Range as LspRange, Url};

    fn make_app() -> App {
        let mut app = App::new();
        app.add_message::<LspDiagnosticsUpdated>();
        app.add_systems(Update, (on_lsp_diagnostics, sync_lsp_glyph_markers).chain());
        app
    }

    fn diag(line: u32, severity: DiagnosticSeverity, message: &str) -> Diagnostic {
        Diagnostic {
            range: LspRange {
                start: Position { line, character: 0 },
                end: Position { line, character: 5 },
            },
            severity: Some(severity),
            code: None,
            code_description: None,
            source: None,
            message: message.to_string(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    fn fake_uri() -> Url {
        Url::parse("file:///fake.rs").unwrap()
    }

    /// Spawn just the components both systems read from the editor entity.
    /// `on_lsp_diagnostics` reads `(RenderSettings, Misc)`;
    /// `sync_lsp_glyph_markers` reads `(DiagnosticColors, &mut GlyphMarkers,
    /// &mut GutterDecorations)` filtered by `CodeEditor`.
    fn spawn_editor(app: &mut App) -> Entity {
        app.world_mut()
            .spawn((
                crate::types::CodeEditor,
                RenderSettings::default(),
                Misc::default(),
                crate::settings::DiagnosticColors::default(),
                super::super::markers::GlyphMarkers::default(),
                super::super::bars::GutterDecorations::default(),
            ))
            .id()
    }

    #[test]
    fn warning_diagnostic_becomes_glyph_marker_and_bar() {
        let mut app = make_app();
        let editor = spawn_editor(&mut app);

        app.world_mut().write_message(LspDiagnosticsUpdated {
            entity: editor,
            uri: fake_uri(),
            version: None,
            diagnostics: vec![diag(3, DiagnosticSeverity::WARNING, "unused variable")],
        });
        app.update();

        let markers = app
            .world()
            .entity(editor)
            .get::<super::super::markers::GlyphMarkers>()
            .expect("editor has GlyphMarkers");
        assert_eq!(markers.0.len(), 1, "exactly one severity marker");
        assert_eq!(markers.0[0].line, 3);
        assert_eq!(markers.0[0].kind, GlyphKind::DiagnosticWarning);

        let bars = app
            .world()
            .entity(editor)
            .get::<super::super::bars::GutterDecorations>()
            .expect("editor has GutterDecorations");
        let diag_bars: Vec<_> = bars
            .0
            .iter()
            .filter(|d| matches!(d.kind, DecorationKind::DiagnosticBar))
            .collect();
        assert_eq!(diag_bars.len(), 1, "one diagnostic bar for one warning");
        assert_eq!(diag_bars[0].line, 3);
    }

    #[test]
    fn error_outranks_warning_on_same_line() {
        let mut app = make_app();
        let editor = spawn_editor(&mut app);

        app.world_mut().write_message(LspDiagnosticsUpdated {
            entity: editor,
            uri: fake_uri(),
            version: None,
            diagnostics: vec![
                diag(7, DiagnosticSeverity::WARNING, "w"),
                diag(7, DiagnosticSeverity::ERROR, "e"),
            ],
        });
        app.update();

        let markers = app
            .world()
            .entity(editor)
            .get::<super::super::markers::GlyphMarkers>()
            .unwrap();
        assert_eq!(markers.0.len(), 1);
        assert_eq!(markers.0[0].line, 7);
        assert_eq!(
            markers.0[0].kind,
            GlyphKind::DiagnosticError,
            "error must outrank warning on collision",
        );
    }

    /// `RenderValidationDecorations::Off` short-circuits in `on_lsp_diagnostics`:
    /// the bridge should produce zero markers regardless of incoming diagnostics.
    #[test]
    fn render_validation_off_suppresses_markers() {
        let mut app = make_app();
        let editor = spawn_editor(&mut app);
        {
            let mut editor_mut = app.world_mut().entity_mut(editor);
            let mut render = editor_mut.get_mut::<RenderSettings>().unwrap();
            render.render_validation_decorations = RenderValidationDecorations::Off;
        }

        app.world_mut().write_message(LspDiagnosticsUpdated {
            entity: editor,
            uri: fake_uri(),
            version: None,
            diagnostics: vec![diag(0, DiagnosticSeverity::ERROR, "boom")],
        });
        app.update();

        let markers = app
            .world()
            .entity(editor)
            .get::<super::super::markers::GlyphMarkers>()
            .unwrap();
        assert_eq!(
            markers.0.len(),
            0,
            "Off mode must suppress all diagnostic markers",
        );
        let diag_marker_count = app
            .world_mut()
            .query::<&DiagnosticMarker>()
            .iter(app.world())
            .count();
        assert_eq!(
            diag_marker_count, 0,
            "no DiagnosticMarker entities should be spawned in Off mode",
        );
    }

    /// `Editable` + `read_only: true` means decorations are suppressed (matches
    /// VSCode behaviour where validation decorations stay hidden in non-editable
    /// views unless forced On).
    #[test]
    fn editable_mode_suppresses_when_read_only() {
        let mut app = make_app();
        let editor = spawn_editor(&mut app);
        {
            let mut editor_mut = app.world_mut().entity_mut(editor);
            editor_mut.get_mut::<Misc>().unwrap().read_only = true;
        }

        app.world_mut().write_message(LspDiagnosticsUpdated {
            entity: editor,
            uri: fake_uri(),
            version: None,
            diagnostics: vec![diag(1, DiagnosticSeverity::WARNING, "w")],
        });
        app.update();

        let markers = app
            .world()
            .entity(editor)
            .get::<super::super::markers::GlyphMarkers>()
            .unwrap();
        assert_eq!(
            markers.0.len(),
            0,
            "Editable + read_only=true must hide decorations",
        );
    }
}
