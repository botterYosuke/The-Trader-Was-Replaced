use std::sync::Arc;

use async_lsp::ServerSocket;
use lsp_types::notification::{
    Cancel as CancelNotif, DidChangeConfiguration, DidChangeTextDocument, DidChangeWatchedFiles,
    DidChangeWorkspaceFolders, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Exit as ExitNotif, WillSaveTextDocument, WorkDoneProgressCancel,
};
use lsp_types::request::{
    CallHierarchyIncomingCalls, CallHierarchyOutgoingCalls, CallHierarchyPrepare,
    CodeActionRequest, CodeActionResolveRequest, ColorPresentationRequest,
    Completion, DocumentColor, DocumentDiagnosticRequest, DocumentHighlightRequest,
    DocumentLinkRequest, DocumentLinkResolve, DocumentSymbolRequest, ExecuteCommand,
    FoldingRangeRequest, Formatting, GotoDeclaration, GotoDefinition, GotoImplementation,
    GotoTypeDefinition, HoverRequest, InlayHintRequest, InlayHintResolveRequest, LinkedEditingRange,
    MonikerRequest, OnTypeFormatting, PrepareRenameRequest, RangeFormatting, References, Rename,
    ResolveCompletionItem, SelectionRangeRequest, SemanticTokensFullDeltaRequest,
    SemanticTokensFullRequest, SemanticTokensRangeRequest, Shutdown as ShutdownRequest,
    SignatureHelpRequest, TypeHierarchyPrepare, TypeHierarchySubtypes, TypeHierarchySupertypes,
    WillSaveWaitUntil, WorkspaceDiagnosticRequest, WorkspaceSymbolRequest, WorkspaceSymbolResolve,
};
use lsp_types::*;

use crate::client::{emit, fire, fulfill_slot, spawn, text_pos, InboundReplySlots, Tx};
use crate::messages::{CodeActionOrCommand, LspMessage, LspResponse, WorkspaceSymbolResponseItem};

pub(crate) fn dispatch(
    server: &ServerSocket,
    tx: &Tx,
    slots: &Arc<InboundReplySlots>,
    message: LspMessage,
) {
    use LspMessage as M;
    match message {
        M::Initialize { .. } | M::Initialized => {}

        M::CancelRequest { id } => fire::<CancelNotif>(
            server,
            CancelParams {
                id: NumberOrString::Number(id as i32),
            },
        ),
        M::WorkDoneProgressCancel { token } => {
            fire::<WorkDoneProgressCancel>(server, WorkDoneProgressCancelParams { token })
        }

        M::DidOpen {
            uri,
            language_id,
            version,
            text,
        } => fire::<DidOpenTextDocument>(
            server,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id,
                    version,
                    text,
                },
            },
        ),
        M::DidChange {
            uri,
            version,
            changes,
        } => fire::<DidChangeTextDocument>(
            server,
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier { uri, version },
                content_changes: changes,
            },
        ),
        M::DidSave { uri, text } => fire::<DidSaveTextDocument>(
            server,
            DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
                text,
            },
        ),
        M::DidClose { uri } => fire::<DidCloseTextDocument>(
            server,
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
            },
        ),
        M::WillSave { uri, reason } => fire::<WillSaveTextDocument>(
            server,
            WillSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
                reason,
            },
        ),
        M::WillSaveWaitUntil { uri, reason, id } => {
            let params = WillSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
                reason,
            };
            spawn::<WillSaveWaitUntil>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::WillSaveWaitUntil {
                        id,
                        edits: result.unwrap_or_default(),
                    },
                );
            });
        }

        M::DidChangeConfiguration { settings } => {
            fire::<DidChangeConfiguration>(server, DidChangeConfigurationParams { settings })
        }
        M::DidChangeWatchedFiles { changes } => {
            fire::<DidChangeWatchedFiles>(server, DidChangeWatchedFilesParams { changes })
        }
        M::DidChangeWorkspaceFolders { event } => {
            fire::<DidChangeWorkspaceFolders>(server, DidChangeWorkspaceFoldersParams { event })
        }

        M::Completion { uri, position, id } => completion(server, tx, uri, position, id),
        M::ResolveCompletionItem { item, id } => {
            spawn::<ResolveCompletionItem>(server, tx, *item, move |result, tx| {
                emit(
                    tx,
                    LspResponse::ResolvedCompletionItem {
                        id,
                        item: Box::new(result),
                    },
                )
            })
        }
        M::Hover { uri, position, id } => hover(server, tx, uri, position, id),
        M::SignatureHelp { uri, position, id } => signature_help(server, tx, uri, position, id),

        M::GotoDeclaration { uri, position, id } => {
            let params = GotoDefinitionParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<GotoDeclaration>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::Declaration {
                        id,
                        locations: flatten_goto(result),
                    },
                );
            });
        }
        M::GotoDefinition { uri, position, id } => {
            let params = GotoDefinitionParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<GotoDefinition>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::Definition {
                        id,
                        locations: flatten_goto(result),
                    },
                );
            });
        }
        M::GotoTypeDefinition { uri, position, id } => {
            let params = GotoDefinitionParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<GotoTypeDefinition>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::TypeDefinition {
                        id,
                        locations: flatten_goto(result),
                    },
                );
            });
        }
        M::GotoImplementation { uri, position, id } => {
            let params = GotoDefinitionParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<GotoImplementation>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::Implementation {
                        id,
                        locations: flatten_goto(result),
                    },
                );
            });
        }
        M::References { uri, position, id } => {
            let params = ReferenceParams {
                text_document_position: text_pos(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            };
            spawn::<References>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::References {
                        id,
                        locations: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::DocumentHighlight { uri, position, id } => {
            let params = DocumentHighlightParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<DocumentHighlightRequest>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::DocumentHighlights {
                        id,
                        highlights: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::DocumentSymbol { uri, id } => {
            let params = DocumentSymbolParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<DocumentSymbolRequest>(server, tx, params, move |result, tx| {
                let (flat, nested) = match result {
                    Some(DocumentSymbolResponse::Flat(items)) => (items, Vec::new()),
                    Some(DocumentSymbolResponse::Nested(items)) => (Vec::new(), items),
                    None => (Vec::new(), Vec::new()),
                };
                emit(tx, LspResponse::DocumentSymbols { id, flat, nested });
            });
        }
        M::WorkspaceSymbol { query, id } => {
            let params = WorkspaceSymbolParams {
                query,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<WorkspaceSymbolRequest>(server, tx, params, move |result, tx| {
                let symbols = match result {
                    Some(WorkspaceSymbolResponse::Flat(items)) => items
                        .into_iter()
                        .map(WorkspaceSymbolResponseItem::Information)
                        .collect(),
                    Some(WorkspaceSymbolResponse::Nested(items)) => items
                        .into_iter()
                        .map(WorkspaceSymbolResponseItem::Symbol)
                        .collect(),
                    None => Vec::new(),
                };
                emit(tx, LspResponse::WorkspaceSymbols { id, symbols });
            });
        }
        M::WorkspaceSymbolResolve { symbol, id } => {
            spawn::<WorkspaceSymbolResolve>(server, tx, symbol, move |result, tx| {
                emit(
                    tx,
                    LspResponse::ResolvedWorkspaceSymbol { id, symbol: result },
                )
            })
        }

        M::FoldingRange { uri, id } => {
            let params = FoldingRangeParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<FoldingRangeRequest>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::FoldingRanges {
                        id,
                        ranges: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::SelectionRange { uri, positions, id } => {
            let params = SelectionRangeParams {
                text_document: TextDocumentIdentifier { uri },
                positions,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<SelectionRangeRequest>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::SelectionRanges {
                        id,
                        ranges: result.unwrap_or_default(),
                    },
                );
            });
        }

        M::CodeAction {
            uri,
            range,
            diagnostics,
            id,
        } => code_action(server, tx, uri, range, diagnostics, id),
        M::CodeActionResolve { action, id } => {
            spawn::<CodeActionResolveRequest>(server, tx, *action, move |result, tx| {
                emit(
                    tx,
                    LspResponse::ResolvedCodeAction {
                        id,
                        action: Box::new(result),
                    },
                )
            })
        }
        M::Format { uri, options, id } => {
            let params = DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri },
                options,
                work_done_progress_params: Default::default(),
            };
            spawn::<Formatting>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::Format {
                        id,
                        edits: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::RangeFormatting {
            uri,
            range,
            options,
            id,
        } => {
            let params = DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier { uri },
                range,
                options,
                work_done_progress_params: Default::default(),
            };
            spawn::<RangeFormatting>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::RangeFormatting {
                        id,
                        edits: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::OnTypeFormatting {
            uri,
            position,
            ch,
            options,
            id,
        } => {
            let params = DocumentOnTypeFormattingParams {
                text_document_position: text_pos(uri, position),
                ch,
                options,
            };
            spawn::<OnTypeFormatting>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::OnTypeFormatting {
                        id,
                        edits: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::ExecuteCommand { command, arguments } => execute_command(server, tx, command, arguments),

        M::InlayHint { uri, range, id } => {
            let params = InlayHintParams {
                text_document: TextDocumentIdentifier { uri },
                range,
                work_done_progress_params: Default::default(),
            };
            spawn::<InlayHintRequest>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::InlayHints {
                        id,
                        hints: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::InlayHintResolve { hint, id } => {
            spawn::<InlayHintResolveRequest>(server, tx, hint, move |result, tx| {
                emit(tx, LspResponse::ResolvedInlayHint { id, hint: result })
            })
        }
        M::DocumentLink { uri, id } => {
            let params = DocumentLinkParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<DocumentLinkRequest>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::DocumentLinks {
                        id,
                        links: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::DocumentLinkResolve { link, id } => {
            spawn::<DocumentLinkResolve>(server, tx, link, move |result, tx| {
                emit(tx, LspResponse::ResolvedDocumentLink { id, link: result })
            })
        }
        M::DocumentColor { uri, id } => {
            let params = DocumentColorParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<DocumentColor>(server, tx, params, move |result, tx| {
                emit(tx, LspResponse::DocumentColors { id, colors: result });
            });
        }
        M::ColorPresentation {
            uri,
            color,
            range,
            id,
        } => {
            let params = ColorPresentationParams {
                text_document: TextDocumentIdentifier { uri },
                color,
                range,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<ColorPresentationRequest>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::ColorPresentations {
                        id,
                        presentations: result,
                    },
                );
            });
        }
        M::LinkedEditingRange { uri, position, id } => {
            let params = LinkedEditingRangeParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
            };
            spawn::<LinkedEditingRange>(server, tx, params, move |result, tx| {
                emit(tx, LspResponse::LinkedEditingRanges { id, ranges: result });
            });
        }
        M::Moniker { uri, position, id } => {
            let params = MonikerParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<MonikerRequest>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::Monikers {
                        id,
                        monikers: result.unwrap_or_default(),
                    },
                );
            });
        }

        M::PrepareRename { uri, position, id } => prepare_rename(server, tx, uri, position, id),
        M::Rename {
            uri,
            position,
            new_name,
            id,
        } => {
            let params = RenameParams {
                text_document_position: text_pos(uri, position),
                new_name,
                work_done_progress_params: Default::default(),
            };
            spawn::<Rename>(server, tx, params, move |result, tx| {
                if let Some(edit) = result {
                    emit(tx, LspResponse::Rename { id, edit });
                }
            });
        }

        M::PrepareCallHierarchy { uri, position, id } => {
            let params = CallHierarchyPrepareParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
            };
            spawn::<CallHierarchyPrepare>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::PrepareCallHierarchy {
                        id,
                        items: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::CallHierarchyIncomingCalls { item, id } => {
            let params = CallHierarchyIncomingCallsParams {
                item,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<CallHierarchyIncomingCalls>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::CallHierarchyIncomingCalls {
                        id,
                        calls: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::CallHierarchyOutgoingCalls { item, id } => {
            let params = CallHierarchyOutgoingCallsParams {
                item,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<CallHierarchyOutgoingCalls>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::CallHierarchyOutgoingCalls {
                        id,
                        calls: result.unwrap_or_default(),
                    },
                );
            });
        }

        M::PrepareTypeHierarchy { uri, position, id } => {
            let params = TypeHierarchyPrepareParams {
                text_document_position_params: text_pos(uri, position),
                work_done_progress_params: Default::default(),
            };
            spawn::<TypeHierarchyPrepare>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::PrepareTypeHierarchy {
                        id,
                        items: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::TypeHierarchySupertypes { item, id } => {
            let params = TypeHierarchySupertypesParams {
                item,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<TypeHierarchySupertypes>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::TypeHierarchySupertypes {
                        id,
                        items: result.unwrap_or_default(),
                    },
                );
            });
        }
        M::TypeHierarchySubtypes { item, id } => {
            let params = TypeHierarchySubtypesParams {
                item,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<TypeHierarchySubtypes>(server, tx, params, move |result, tx| {
                emit(
                    tx,
                    LspResponse::TypeHierarchySubtypes {
                        id,
                        items: result.unwrap_or_default(),
                    },
                );
            });
        }

        M::SemanticTokensFull { uri, id } => {
            let params = SemanticTokensParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<SemanticTokensFullRequest>(server, tx, params, move |result, tx| {
                if let Some(result) = result {
                    emit(tx, LspResponse::SemanticTokens { id, result });
                }
            });
        }
        M::SemanticTokensFullDelta {
            uri,
            previous_result_id,
            id,
        } => {
            let params = SemanticTokensDeltaParams {
                text_document: TextDocumentIdentifier { uri },
                previous_result_id,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<SemanticTokensFullDeltaRequest>(server, tx, params, move |result, tx| {
                if let Some(result) = result {
                    emit(tx, LspResponse::SemanticTokensDelta { id, result });
                }
            });
        }
        M::SemanticTokensRange { uri, range, id } => {
            let params = SemanticTokensRangeParams {
                text_document: TextDocumentIdentifier { uri },
                range,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<SemanticTokensRangeRequest>(server, tx, params, move |result, tx| {
                if let Some(result) = result {
                    emit(tx, LspResponse::SemanticTokensRange { id, result });
                }
            });
        }

        M::DocumentDiagnostic {
            uri,
            identifier,
            previous_result_id,
            id,
        } => {
            let params = DocumentDiagnosticParams {
                text_document: TextDocumentIdentifier { uri },
                identifier,
                previous_result_id,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<DocumentDiagnosticRequest>(server, tx, params, move |result, tx| {
                emit(tx, LspResponse::DocumentDiagnostic { id, report: result });
            });
        }
        M::WorkspaceDiagnostic {
            identifier,
            previous_result_ids,
            id,
        } => {
            let params = WorkspaceDiagnosticParams {
                identifier,
                previous_result_ids,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            spawn::<WorkspaceDiagnosticRequest>(server, tx, params, move |result, tx| {
                emit(tx, LspResponse::WorkspaceDiagnostic { id, report: result });
            });
        }

        M::RespondConfiguration { id, items } => fulfill_slot(&slots.configuration, id, items),
        M::RespondApplyEdit { id, response } => fulfill_slot(&slots.apply_edit, id, response),
        M::RespondShowMessageRequest { id, action } => {
            fulfill_slot(&slots.show_message, id, action)
        }
        M::RespondShowDocument { id, success } => {
            fulfill_slot(&slots.show_document, id, ShowDocumentResult { success })
        }
        M::RespondWorkDoneProgressCreate { id } => {
            fulfill_slot(&slots.work_done_progress_create, id, ())
        }
        M::RespondRegisterCapability { id } => fulfill_slot(&slots.register_capability, id, ()),
        M::RespondUnregisterCapability { id } => fulfill_slot(&slots.unregister_capability, id, ()),
        M::RespondWorkspaceFolders { id, folders } => {
            fulfill_slot(&slots.workspace_folders, id, folders)
        }

        M::Shutdown { id } => spawn::<ShutdownRequest>(server, tx, (), move |_result, tx| {
            emit(tx, LspResponse::ShutdownAck { id });
        }),
        M::Exit => fire::<ExitNotif>(server, ()),
    }
}

fn completion(server: &ServerSocket, tx: &Tx, uri: Url, position: Position, id: u64) {
    let params = CompletionParams {
        text_document_position: text_pos(uri, position),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    spawn::<Completion>(server, tx, params, move |result, tx| match result {
        Some(CompletionResponse::Array(items)) => emit(
            tx,
            LspResponse::Completion {
                id,
                items,
                is_incomplete: false,
            },
        ),
        Some(CompletionResponse::List(list)) => emit(
            tx,
            LspResponse::Completion {
                id,
                items: list.items,
                is_incomplete: list.is_incomplete,
            },
        ),
        None => emit(
            tx,
            LspResponse::Completion {
                id,
                items: Vec::new(),
                is_incomplete: false,
            },
        ),
    });
}

fn hover(server: &ServerSocket, tx: &Tx, uri: Url, position: Position, id: u64) {
    let params = HoverParams {
        text_document_position_params: text_pos(uri, position),
        work_done_progress_params: Default::default(),
    };
    spawn::<HoverRequest>(server, tx, params, move |result, tx| {
        if let Some(h) = result {
            let (content, kind) = extract_hover_content(&h.contents);
            emit(
                tx,
                LspResponse::Hover {
                    id,
                    content,
                    kind,
                    range: h.range,
                },
            );
        }
    });
}

fn signature_help(server: &ServerSocket, tx: &Tx, uri: Url, position: Position, id: u64) {
    let params = SignatureHelpParams {
        text_document_position_params: text_pos(uri, position),
        work_done_progress_params: Default::default(),
        context: None,
    };
    spawn::<SignatureHelpRequest>(server, tx, params, move |result, tx| {
        let (signatures, active_signature, active_parameter) = match result {
            Some(sig) => (sig.signatures, sig.active_signature, sig.active_parameter),
            None => (Vec::new(), None, None),
        };
        emit(
            tx,
            LspResponse::SignatureHelp {
                id,
                signatures,
                active_signature,
                active_parameter,
            },
        );
    });
}

fn code_action(
    server: &ServerSocket,
    tx: &Tx,
    uri: Url,
    range: Range,
    diagnostics: Vec<Diagnostic>,
    id: u64,
) {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range,
        context: CodeActionContext {
            diagnostics,
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    spawn::<CodeActionRequest>(server, tx, params, move |result, tx| {
        let actions = result
            .unwrap_or_default()
            .into_iter()
            .map(|a| match a {
                lsp_types::CodeActionOrCommand::CodeAction(a) => {
                    CodeActionOrCommand::Action(Box::new(a))
                }
                lsp_types::CodeActionOrCommand::Command(c) => CodeActionOrCommand::Command(c),
            })
            .collect();
        emit(tx, LspResponse::CodeActions { id, actions });
    });
}

fn execute_command(
    server: &ServerSocket,
    tx: &Tx,
    command: String,
    arguments: Option<Vec<serde_json::Value>>,
) {
    let params = ExecuteCommandParams {
        command,
        arguments: arguments.unwrap_or_default(),
        work_done_progress_params: Default::default(),
    };
    spawn::<ExecuteCommand>(server, tx, params, |_result, _tx| {});
}

fn prepare_rename(server: &ServerSocket, tx: &Tx, uri: Url, position: Position, id: u64) {
    spawn::<PrepareRenameRequest>(server, tx, text_pos(uri, position), move |result, tx| {
        match result {
            Some(PrepareRenameResponse::Range(range)) => emit(
                tx,
                LspResponse::PrepareRename {
                    id,
                    range,
                    placeholder: None,
                },
            ),
            Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) => emit(
                tx,
                LspResponse::PrepareRename {
                    id,
                    range,
                    placeholder: Some(placeholder),
                },
            ),
            // DefaultBehavior needs identifier-at-cursor, which the protocol layer can't compute.
            Some(PrepareRenameResponse::DefaultBehavior { .. }) | None => {}
        }
    });
}

fn flatten_goto(r: Option<GotoDefinitionResponse>) -> Vec<Location> {
    match r {
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
        None => Vec::new(),
    }
}

fn extract_hover_content(contents: &HoverContents) -> (String, MarkupKind) {
    match contents {
        HoverContents::Markup(markup) => (markup.value.clone(), markup.kind.clone()),
        HoverContents::Scalar(s) => marked_string_with_kind(s),
        HoverContents::Array(arr) => {
            let any_lang = arr
                .iter()
                .any(|ms| matches!(ms, MarkedString::LanguageString(_)));
            let mut out = String::new();
            for (i, ms) in arr.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                out.push_str(&render_marked_string(ms, any_lang));
            }
            let kind = if any_lang {
                MarkupKind::Markdown
            } else {
                MarkupKind::PlainText
            };
            (out, kind)
        }
    }
}

fn marked_string_with_kind(ms: &MarkedString) -> (String, MarkupKind) {
    match ms {
        MarkedString::String(s) => (s.clone(), MarkupKind::PlainText),
        MarkedString::LanguageString(ls) => (
            format!("```{}\n{}\n```", ls.language, ls.value),
            MarkupKind::Markdown,
        ),
    }
}

fn render_marked_string(ms: &MarkedString, force_markdown: bool) -> String {
    match ms {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(ls) => {
            if force_markdown {
                format!("```{}\n{}\n```", ls.language, ls.value)
            } else {
                ls.value.clone()
            }
        }
    }
}
