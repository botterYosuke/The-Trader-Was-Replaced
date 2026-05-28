//! Bevy plugin that drains LSP transport traffic into per-variant ECS messages.

use bevy_app::{App, AppExit, Last, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use bevy_log::{debug, info, trace};

use crate::client::LspClient;
use crate::document::LspDocument;
use crate::messages::{LspRequest, *};

#[derive(Default)]
pub struct LspPlugin;

impl Plugin for LspPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LspServerInitialized>()
            .add_message::<LspDiagnosticsUpdated>()
            .add_message::<LspLogMessage>()
            .add_message::<LspShowMessage>()
            .add_message::<LspProgress>()
            .add_message::<LspTelemetry>()
            .add_message::<LspLogTrace>()
            .add_message::<LspConfigurationRequested>()
            .add_message::<LspApplyEditRequested>()
            .add_message::<LspShowMessageRequestRequested>()
            .add_message::<LspShowDocumentRequested>()
            .add_message::<LspWorkDoneProgressCreateRequested>()
            .add_message::<LspRegisterCapabilityRequested>()
            .add_message::<LspUnregisterCapabilityRequested>()
            .add_message::<LspWorkspaceFoldersRequested>()
            .add_message::<LspSemanticTokensRefreshRequested>()
            .add_message::<LspInlayHintRefreshRequested>()
            .add_message::<LspCodeLensRefreshRequested>()
            .add_message::<LspDiagnosticsRefreshRequested>()
            .add_message::<LspCompletionResponse>()
            .add_message::<LspResolvedCompletionItem>()
            .add_message::<LspHoverResponse>()
            .add_message::<LspSignatureHelpResponse>()
            .add_message::<LspDeclarationResponse>()
            .add_message::<LspDefinitionResponse>()
            .add_message::<LspTypeDefinitionResponse>()
            .add_message::<LspImplementationResponse>()
            .add_message::<LspReferencesResponse>()
            .add_message::<LspDocumentHighlightsResponse>()
            .add_message::<LspDocumentSymbolsResponse>()
            .add_message::<LspWorkspaceSymbolsResponse>()
            .add_message::<LspResolvedWorkspaceSymbol>()
            .add_message::<LspFoldingRangesResponse>()
            .add_message::<LspSelectionRangesResponse>()
            .add_message::<LspCodeActionsResponse>()
            .add_message::<LspResolvedCodeAction>()
            .add_message::<LspFormatResponse>()
            .add_message::<LspRangeFormattingResponse>()
            .add_message::<LspOnTypeFormattingResponse>()
            .add_message::<LspWillSaveWaitUntilResponse>()
            .add_message::<LspInlayHintsResponse>()
            .add_message::<LspResolvedInlayHint>()
            .add_message::<LspDocumentLinksResponse>()
            .add_message::<LspResolvedDocumentLink>()
            .add_message::<LspDocumentColorsResponse>()
            .add_message::<LspColorPresentationsResponse>()
            .add_message::<LspLinkedEditingRangesResponse>()
            .add_message::<LspMonikersResponse>()
            .add_message::<LspPrepareRenameResponse>()
            .add_message::<LspRenameResponse>()
            .add_message::<LspPrepareCallHierarchyResponse>()
            .add_message::<LspCallHierarchyIncomingCallsResponse>()
            .add_message::<LspCallHierarchyOutgoingCallsResponse>()
            .add_message::<LspPrepareTypeHierarchyResponse>()
            .add_message::<LspTypeHierarchySupertypesResponse>()
            .add_message::<LspTypeHierarchySubtypesResponse>()
            .add_message::<LspSemanticTokensResponse>()
            .add_message::<LspSemanticTokensDeltaResponse>()
            .add_message::<LspSemanticTokensRangeResponse>()
            .add_message::<LspDocumentDiagnosticResponse>()
            .add_message::<LspWorkspaceDiagnosticResponse>()
            .add_message::<LspShutdownAck>()
            .add_message::<LspServerCrashed>();

        app.add_message::<LspRequest>();
        app.add_systems(Update, dispatch_lsp_requests);

        app.add_systems(Update, (flush_document_changes, drain_lsp_responses));
        app.add_systems(Last, shutdown_clients_on_app_exit);
    }
}

#[derive(SystemParam)]
struct LspResponseWriters<'w> {
    initialized: MessageWriter<'w, LspServerInitialized>,
    diagnostics: MessageWriter<'w, LspDiagnosticsUpdated>,
    log_message: MessageWriter<'w, LspLogMessage>,
    show_message: MessageWriter<'w, LspShowMessage>,
    progress: MessageWriter<'w, LspProgress>,
    telemetry: MessageWriter<'w, LspTelemetry>,
    log_trace: MessageWriter<'w, LspLogTrace>,

    configuration_req: MessageWriter<'w, LspConfigurationRequested>,
    apply_edit_req: MessageWriter<'w, LspApplyEditRequested>,
    show_message_req: MessageWriter<'w, LspShowMessageRequestRequested>,
    show_document_req: MessageWriter<'w, LspShowDocumentRequested>,
    work_done_create_req: MessageWriter<'w, LspWorkDoneProgressCreateRequested>,
    register_cap_req: MessageWriter<'w, LspRegisterCapabilityRequested>,
    unregister_cap_req: MessageWriter<'w, LspUnregisterCapabilityRequested>,
    workspace_folders_req: MessageWriter<'w, LspWorkspaceFoldersRequested>,

    semantic_refresh_req: MessageWriter<'w, LspSemanticTokensRefreshRequested>,
    inlay_refresh_req: MessageWriter<'w, LspInlayHintRefreshRequested>,
    code_lens_refresh_req: MessageWriter<'w, LspCodeLensRefreshRequested>,
    diagnostics_refresh_req: MessageWriter<'w, LspDiagnosticsRefreshRequested>,

    completion: MessageWriter<'w, LspCompletionResponse>,
    resolved_completion: MessageWriter<'w, LspResolvedCompletionItem>,
    hover: MessageWriter<'w, LspHoverResponse>,
    signature_help: MessageWriter<'w, LspSignatureHelpResponse>,

    declaration: MessageWriter<'w, LspDeclarationResponse>,
    definition: MessageWriter<'w, LspDefinitionResponse>,
    type_definition: MessageWriter<'w, LspTypeDefinitionResponse>,
    implementation: MessageWriter<'w, LspImplementationResponse>,
    references: MessageWriter<'w, LspReferencesResponse>,
    highlights: MessageWriter<'w, LspDocumentHighlightsResponse>,
    document_symbols: MessageWriter<'w, LspDocumentSymbolsResponse>,
    workspace_symbols: MessageWriter<'w, LspWorkspaceSymbolsResponse>,
    resolved_workspace_symbol: MessageWriter<'w, LspResolvedWorkspaceSymbol>,
    folding: MessageWriter<'w, LspFoldingRangesResponse>,
    selection: MessageWriter<'w, LspSelectionRangesResponse>,

    code_actions: MessageWriter<'w, LspCodeActionsResponse>,
    resolved_code_action: MessageWriter<'w, LspResolvedCodeAction>,
    format: MessageWriter<'w, LspFormatResponse>,
    range_formatting: MessageWriter<'w, LspRangeFormattingResponse>,
    on_type_formatting: MessageWriter<'w, LspOnTypeFormattingResponse>,
    will_save_wait: MessageWriter<'w, LspWillSaveWaitUntilResponse>,

    inlay: MessageWriter<'w, LspInlayHintsResponse>,
    resolved_inlay: MessageWriter<'w, LspResolvedInlayHint>,
    doc_links: MessageWriter<'w, LspDocumentLinksResponse>,
    resolved_doc_link: MessageWriter<'w, LspResolvedDocumentLink>,
    doc_colors: MessageWriter<'w, LspDocumentColorsResponse>,
    color_presentations: MessageWriter<'w, LspColorPresentationsResponse>,
    linked_editing: MessageWriter<'w, LspLinkedEditingRangesResponse>,
    monikers: MessageWriter<'w, LspMonikersResponse>,

    prepare_rename: MessageWriter<'w, LspPrepareRenameResponse>,
    rename: MessageWriter<'w, LspRenameResponse>,

    prepare_call: MessageWriter<'w, LspPrepareCallHierarchyResponse>,
    incoming_calls: MessageWriter<'w, LspCallHierarchyIncomingCallsResponse>,
    outgoing_calls: MessageWriter<'w, LspCallHierarchyOutgoingCallsResponse>,
    prepare_type: MessageWriter<'w, LspPrepareTypeHierarchyResponse>,
    super_types: MessageWriter<'w, LspTypeHierarchySupertypesResponse>,
    sub_types: MessageWriter<'w, LspTypeHierarchySubtypesResponse>,

    semantic_tokens: MessageWriter<'w, LspSemanticTokensResponse>,
    semantic_delta: MessageWriter<'w, LspSemanticTokensDeltaResponse>,
    semantic_range: MessageWriter<'w, LspSemanticTokensRangeResponse>,
    doc_diagnostic: MessageWriter<'w, LspDocumentDiagnosticResponse>,
    ws_diagnostic: MessageWriter<'w, LspWorkspaceDiagnosticResponse>,

    shutdown_ack: MessageWriter<'w, LspShutdownAck>,
    crashed: MessageWriter<'w, LspServerCrashed>,
}

fn response_variant_name(r: &crate::messages::LspResponse) -> &'static str {
    use crate::messages::LspResponse as R;
    match r {
        R::Initialized { .. } => "Initialized",
        R::Diagnostics { .. } => "Diagnostics",
        R::LogMessage { .. } => "LogMessage",
        R::ShowMessage { .. } => "ShowMessage",
        R::Progress { .. } => "Progress",
        R::Telemetry { .. } => "Telemetry",
        R::LogTrace { .. } => "LogTrace",
        R::ConfigurationRequested { .. } => "ConfigurationRequested",
        R::ApplyEditRequested { .. } => "ApplyEditRequested",
        R::ShowMessageRequestRequested { .. } => "ShowMessageRequestRequested",
        R::WorkspaceFoldersRequested { .. } => "WorkspaceFoldersRequested",
        R::RegisterCapabilityRequested { .. } => "RegisterCapabilityRequested",
        R::UnregisterCapabilityRequested { .. } => "UnregisterCapabilityRequested",
        R::WorkDoneProgressCreateRequested { .. } => "WorkDoneProgressCreateRequested",
        R::ShutdownAck { .. } => "ShutdownAck",
        R::Crashed => "Crashed",
        _ => "Other",
    }
}

fn drain_lsp_responses(mut clients: Query<(Entity, &mut LspClient)>, mut w: LspResponseWriters) {
    use crate::messages::LspResponse as R;
    for (entity, mut client) in clients.iter_mut() {
        client.cleanup_timeouts();
        while let Some(response) = client.try_recv() {
            trace!(
                "[LSP] drain entity={entity} response={}",
                response_variant_name(&response),
            );
            match response {
                R::Initialized { capabilities } => {
                    client.initialized = true;
                    for msg in std::mem::take(&mut client.pre_init_queue) {
                        client.send(msg);
                    }
                    w.initialized.write(LspServerInitialized {
                        entity,
                        capabilities: *capabilities,
                    });
                }
                R::Diagnostics {
                    uri,
                    version,
                    diagnostics,
                } => {
                    if !diagnostics.is_empty() {
                        debug!(
                            "[LSP] pump R::Diagnostics entity={entity} uri={uri} version={version:?} count={}",
                            diagnostics.len(),
                        );
                    }
                    w.diagnostics.write(LspDiagnosticsUpdated {
                        entity,
                        uri,
                        version,
                        diagnostics,
                    });
                }
                R::LogMessage { typ, message } => {
                    w.log_message.write(LspLogMessage {
                        entity,
                        typ,
                        message,
                    });
                }
                R::ShowMessage { typ, message } => {
                    w.show_message.write(LspShowMessage {
                        entity,
                        typ,
                        message,
                    });
                }
                R::Progress { token, value } => {
                    w.progress.write(LspProgress {
                        entity,
                        token,
                        value,
                    });
                }
                R::Telemetry { data } => {
                    w.telemetry.write(LspTelemetry { entity, data });
                }
                R::LogTrace { message, verbose } => {
                    w.log_trace.write(LspLogTrace {
                        entity,
                        message,
                        verbose,
                    });
                }
                R::ConfigurationRequested { request_id, items } => {
                    w.configuration_req.write(LspConfigurationRequested {
                        entity,
                        request_id,
                        items,
                    });
                }
                R::ApplyEditRequested {
                    request_id,
                    label,
                    edit,
                } => {
                    w.apply_edit_req.write(LspApplyEditRequested {
                        entity,
                        request_id,
                        label,
                        edit,
                    });
                }
                R::ShowMessageRequestRequested {
                    request_id,
                    typ,
                    message,
                    actions,
                } => {
                    w.show_message_req.write(LspShowMessageRequestRequested {
                        entity,
                        request_id,
                        typ,
                        message,
                        actions,
                    });
                }
                R::ShowDocumentRequested {
                    request_id,
                    uri,
                    external,
                    take_focus,
                    selection,
                } => {
                    w.show_document_req.write(LspShowDocumentRequested {
                        entity,
                        request_id,
                        uri,
                        external,
                        take_focus,
                        selection,
                    });
                }
                R::WorkDoneProgressCreateRequested { request_id, token } => {
                    w.work_done_create_req
                        .write(LspWorkDoneProgressCreateRequested {
                            entity,
                            request_id,
                            token,
                        });
                }
                R::RegisterCapabilityRequested {
                    request_id,
                    registrations,
                } => {
                    w.register_cap_req.write(LspRegisterCapabilityRequested {
                        entity,
                        request_id,
                        registrations,
                    });
                }
                R::UnregisterCapabilityRequested {
                    request_id,
                    unregistrations,
                } => {
                    w.unregister_cap_req
                        .write(LspUnregisterCapabilityRequested {
                            entity,
                            request_id,
                            unregistrations,
                        });
                }
                R::WorkspaceFoldersRequested { request_id } => {
                    w.workspace_folders_req
                        .write(LspWorkspaceFoldersRequested { entity, request_id });
                }
                R::SemanticTokensRefreshRequested => {
                    w.semantic_refresh_req
                        .write(LspSemanticTokensRefreshRequested { entity });
                }
                R::InlayHintRefreshRequested => {
                    w.inlay_refresh_req
                        .write(LspInlayHintRefreshRequested { entity });
                }
                R::CodeLensRefreshRequested => {
                    w.code_lens_refresh_req
                        .write(LspCodeLensRefreshRequested { entity });
                }
                R::DiagnosticsRefreshRequested => {
                    w.diagnostics_refresh_req
                        .write(LspDiagnosticsRefreshRequested { entity });
                }
                R::Completion {
                    id,
                    items,
                    is_incomplete,
                } => {
                    w.completion.write(LspCompletionResponse {
                        entity,
                        id,
                        items,
                        is_incomplete,
                    });
                }
                R::ResolvedCompletionItem { id, item } => {
                    w.resolved_completion.write(LspResolvedCompletionItem {
                        entity,
                        id,
                        item: *item,
                    });
                }
                R::Hover {
                    id,
                    content,
                    kind,
                    range,
                } => {
                    w.hover.write(LspHoverResponse {
                        entity,
                        id,
                        content,
                        kind,
                        range,
                    });
                }
                R::SignatureHelp {
                    id,
                    signatures,
                    active_signature,
                    active_parameter,
                } => {
                    w.signature_help.write(LspSignatureHelpResponse {
                        entity,
                        id,
                        signatures,
                        active_signature,
                        active_parameter,
                    });
                }
                R::Declaration { id, locations } => {
                    w.declaration.write(LspDeclarationResponse {
                        entity,
                        id,
                        locations,
                    });
                }
                R::Definition { id, locations } => {
                    w.definition.write(LspDefinitionResponse {
                        entity,
                        id,
                        locations,
                    });
                }
                R::TypeDefinition { id, locations } => {
                    w.type_definition.write(LspTypeDefinitionResponse {
                        entity,
                        id,
                        locations,
                    });
                }
                R::Implementation { id, locations } => {
                    w.implementation.write(LspImplementationResponse {
                        entity,
                        id,
                        locations,
                    });
                }
                R::References { id, locations } => {
                    w.references.write(LspReferencesResponse {
                        entity,
                        id,
                        locations,
                    });
                }
                R::DocumentHighlights { id, highlights } => {
                    w.highlights.write(LspDocumentHighlightsResponse {
                        entity,
                        id,
                        highlights,
                    });
                }
                R::DocumentSymbols { id, flat, nested } => {
                    w.document_symbols.write(LspDocumentSymbolsResponse {
                        entity,
                        id,
                        flat,
                        nested,
                    });
                }
                R::WorkspaceSymbols { id, symbols } => {
                    w.workspace_symbols.write(LspWorkspaceSymbolsResponse {
                        entity,
                        id,
                        symbols,
                    });
                }
                R::ResolvedWorkspaceSymbol { id, symbol } => {
                    w.resolved_workspace_symbol
                        .write(LspResolvedWorkspaceSymbol { entity, id, symbol });
                }
                R::FoldingRanges { id, ranges } => {
                    w.folding
                        .write(LspFoldingRangesResponse { entity, id, ranges });
                }
                R::SelectionRanges { id, ranges } => {
                    w.selection
                        .write(LspSelectionRangesResponse { entity, id, ranges });
                }
                R::CodeActions { id, actions } => {
                    w.code_actions.write(LspCodeActionsResponse {
                        entity,
                        id,
                        actions,
                    });
                }
                R::ResolvedCodeAction { id, action } => {
                    w.resolved_code_action.write(LspResolvedCodeAction {
                        entity,
                        id,
                        action: *action,
                    });
                }
                R::Format { id, edits } => {
                    w.format.write(LspFormatResponse { entity, id, edits });
                }
                R::RangeFormatting { id, edits } => {
                    w.range_formatting
                        .write(LspRangeFormattingResponse { entity, id, edits });
                }
                R::OnTypeFormatting { id, edits } => {
                    w.on_type_formatting
                        .write(LspOnTypeFormattingResponse { entity, id, edits });
                }
                R::WillSaveWaitUntil { id, edits } => {
                    w.will_save_wait
                        .write(LspWillSaveWaitUntilResponse { entity, id, edits });
                }
                R::InlayHints { id, hints } => {
                    w.inlay.write(LspInlayHintsResponse { entity, id, hints });
                }
                R::ResolvedInlayHint { id, hint } => {
                    w.resolved_inlay
                        .write(LspResolvedInlayHint { entity, id, hint });
                }
                R::DocumentLinks { id, links } => {
                    w.doc_links
                        .write(LspDocumentLinksResponse { entity, id, links });
                }
                R::ResolvedDocumentLink { id, link } => {
                    w.resolved_doc_link
                        .write(LspResolvedDocumentLink { entity, id, link });
                }
                R::DocumentColors { id, colors } => {
                    w.doc_colors
                        .write(LspDocumentColorsResponse { entity, id, colors });
                }
                R::ColorPresentations { id, presentations } => {
                    w.color_presentations.write(LspColorPresentationsResponse {
                        entity,
                        id,
                        presentations,
                    });
                }
                R::LinkedEditingRanges { id, ranges } => {
                    w.linked_editing
                        .write(LspLinkedEditingRangesResponse { entity, id, ranges });
                }
                R::Monikers { id, monikers } => {
                    w.monikers.write(LspMonikersResponse {
                        entity,
                        id,
                        monikers,
                    });
                }
                R::PrepareRename {
                    id,
                    range,
                    placeholder,
                } => {
                    w.prepare_rename.write(LspPrepareRenameResponse {
                        entity,
                        id,
                        range,
                        placeholder,
                    });
                }
                R::Rename { id, edit } => {
                    w.rename.write(LspRenameResponse { entity, id, edit });
                }
                R::PrepareCallHierarchy { id, items } => {
                    w.prepare_call
                        .write(LspPrepareCallHierarchyResponse { entity, id, items });
                }
                R::CallHierarchyIncomingCalls { id, calls } => {
                    w.incoming_calls
                        .write(LspCallHierarchyIncomingCallsResponse { entity, id, calls });
                }
                R::CallHierarchyOutgoingCalls { id, calls } => {
                    w.outgoing_calls
                        .write(LspCallHierarchyOutgoingCallsResponse { entity, id, calls });
                }
                R::PrepareTypeHierarchy { id, items } => {
                    w.prepare_type
                        .write(LspPrepareTypeHierarchyResponse { entity, id, items });
                }
                R::TypeHierarchySupertypes { id, items } => {
                    w.super_types
                        .write(LspTypeHierarchySupertypesResponse { entity, id, items });
                }
                R::TypeHierarchySubtypes { id, items } => {
                    w.sub_types
                        .write(LspTypeHierarchySubtypesResponse { entity, id, items });
                }
                R::SemanticTokens { id, result } => {
                    w.semantic_tokens
                        .write(LspSemanticTokensResponse { entity, id, result });
                }
                R::SemanticTokensDelta { id, result } => {
                    w.semantic_delta
                        .write(LspSemanticTokensDeltaResponse { entity, id, result });
                }
                R::SemanticTokensRange { id, result } => {
                    w.semantic_range
                        .write(LspSemanticTokensRangeResponse { entity, id, result });
                }
                R::DocumentDiagnostic { id, report } => {
                    w.doc_diagnostic
                        .write(LspDocumentDiagnosticResponse { entity, id, report });
                }
                R::WorkspaceDiagnostic { id, report } => {
                    w.ws_diagnostic
                        .write(LspWorkspaceDiagnosticResponse { entity, id, report });
                }
                R::ShutdownAck { id } => {
                    w.shutdown_ack.write(LspShutdownAck { entity, id });
                }
                R::Crashed => {
                    w.crashed.write(LspServerCrashed { entity });
                }
            }
        }
    }
}

fn flush_document_changes(
    mut query: Query<(Entity, &mut LspDocument), Changed<LspDocument>>,
    mut lsp_w: MessageWriter<LspRequest>,
) {
    for (entity, mut doc) in &mut query {
        let Some((version, changes)) = doc.take_changes() else {
            continue;
        };
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::DidChange {
                uri: doc.uri.clone(),
                version,
                changes,
            },
        });
    }
}

fn dispatch_lsp_requests(
    mut requests: MessageReader<LspRequest>,
    mut clients: Query<&mut LspClient>,
) {
    for req in requests.read() {
        let Ok(mut client) = clients.get_mut(req.entity) else {
            info!(
                "[LSP] dispatch_lsp_requests: dropping (entity={} has no LspClient)",
                req.entity,
            );
            continue;
        };
        client.send(req.msg.clone());
    }
}

fn shutdown_clients_on_app_exit(
    mut exit: MessageReader<AppExit>,
    mut clients: Query<&mut LspClient>,
) {
    if exit.read().next().is_none() {
        return;
    }
    for mut client in clients.iter_mut() {
        client.shutdown();
    }
}
