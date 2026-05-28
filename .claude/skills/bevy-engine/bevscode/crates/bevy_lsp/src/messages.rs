//! LSP message types — full LSP 3.17 spec coverage.
//!
//! [`LspMessage`] / [`LspResponse`] flow over the async transport channel.
//! The `Lsp*Response` Bevy `Message` types mirror each variant onto the ECS
//! message bus, tagged with the originating [`Entity`].

use bevy_ecs::prelude::*;
use lsp_types::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestType {
    Initialize,
    Completion,
    CompletionItemResolve,
    Hover,
    GotoDeclaration,
    GotoDefinition,
    GotoTypeDefinition,
    GotoImplementation,
    References,
    Format,
    RangeFormatting,
    OnTypeFormatting,
    SignatureHelp,
    CodeAction,
    CodeActionResolve,
    InlayHint,
    InlayHintResolve,
    DocumentHighlight,
    DocumentSymbol,
    WorkspaceSymbol,
    WorkspaceSymbolResolve,
    FoldingRange,
    SelectionRange,
    DocumentLink,
    DocumentLinkResolve,
    DocumentColor,
    ColorPresentation,
    LinkedEditingRange,
    Moniker,
    PrepareRename,
    Rename,
    PrepareCallHierarchy,
    CallHierarchyIncomingCalls,
    CallHierarchyOutgoingCalls,
    PrepareTypeHierarchy,
    TypeHierarchySupertypes,
    TypeHierarchySubtypes,
    SemanticTokensFull,
    SemanticTokensFullDelta,
    SemanticTokensRange,
    DocumentDiagnostic,
    WorkspaceDiagnostic,
    Shutdown,
}

/// Outgoing message to the language server. Variants with `id` echo it back
/// on their response; variants without are fire-and-forget notifications.
#[derive(Debug, Clone)]
pub enum LspMessage {
    Initialize {
        root_uri: Url,
        capabilities: Box<ClientCapabilities>,
    },

    Initialized,

    CancelRequest {
        id: u64,
    },

    DidOpen {
        uri: Url,
        language_id: String,
        version: i32,
        text: String,
    },

    DidChange {
        uri: Url,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    },

    /// `text` is only `Some` when the server's `save.includeText` is `true`.
    DidSave {
        uri: Url,
        text: Option<String>,
    },

    DidClose {
        uri: Url,
    },

    WillSave {
        uri: Url,
        reason: TextDocumentSaveReason,
    },

    WillSaveWaitUntil {
        uri: Url,
        reason: TextDocumentSaveReason,
        id: u64,
    },

    DidChangeConfiguration {
        settings: serde_json::Value,
    },

    DidChangeWatchedFiles {
        changes: Vec<FileEvent>,
    },

    DidChangeWorkspaceFolders {
        event: WorkspaceFoldersChangeEvent,
    },

    Completion {
        uri: Url,
        position: Position,
        id: u64,
    },

    ResolveCompletionItem {
        item: Box<CompletionItem>,
        id: u64,
    },

    Hover {
        uri: Url,
        position: Position,
        id: u64,
    },

    SignatureHelp {
        uri: Url,
        position: Position,
        id: u64,
    },

    GotoDeclaration {
        uri: Url,
        position: Position,
        id: u64,
    },

    GotoDefinition {
        uri: Url,
        position: Position,
        id: u64,
    },

    GotoTypeDefinition {
        uri: Url,
        position: Position,
        id: u64,
    },

    GotoImplementation {
        uri: Url,
        position: Position,
        id: u64,
    },

    References {
        uri: Url,
        position: Position,
        id: u64,
    },

    DocumentHighlight {
        uri: Url,
        position: Position,
        id: u64,
    },

    DocumentSymbol {
        uri: Url,
        id: u64,
    },

    WorkspaceSymbol {
        query: String,
        id: u64,
    },

    WorkspaceSymbolResolve {
        symbol: WorkspaceSymbol,
        id: u64,
    },

    FoldingRange {
        uri: Url,
        id: u64,
    },

    SelectionRange {
        uri: Url,
        positions: Vec<Position>,
        id: u64,
    },

    CodeAction {
        uri: Url,
        range: Range,
        diagnostics: Vec<Diagnostic>,
        id: u64,
    },

    CodeActionResolve {
        action: Box<CodeAction>,
        id: u64,
    },

    Format {
        uri: Url,
        options: FormattingOptions,
        id: u64,
    },

    RangeFormatting {
        uri: Url,
        range: Range,
        options: FormattingOptions,
        id: u64,
    },

    OnTypeFormatting {
        uri: Url,
        position: Position,
        ch: String,
        options: FormattingOptions,
        id: u64,
    },

    ExecuteCommand {
        command: String,
        arguments: Option<Vec<serde_json::Value>>,
    },

    InlayHint {
        uri: Url,
        range: Range,
        id: u64,
    },

    InlayHintResolve {
        hint: InlayHint,
        id: u64,
    },

    DocumentLink {
        uri: Url,
        id: u64,
    },

    DocumentLinkResolve {
        link: DocumentLink,
        id: u64,
    },

    DocumentColor {
        uri: Url,
        id: u64,
    },

    ColorPresentation {
        uri: Url,
        color: lsp_types::Color,
        range: Range,
        id: u64,
    },

    LinkedEditingRange {
        uri: Url,
        position: Position,
        id: u64,
    },

    Moniker {
        uri: Url,
        position: Position,
        id: u64,
    },

    PrepareRename {
        uri: Url,
        position: Position,
        id: u64,
    },

    Rename {
        uri: Url,
        position: Position,
        new_name: String,
        id: u64,
    },

    PrepareCallHierarchy {
        uri: Url,
        position: Position,
        id: u64,
    },

    CallHierarchyIncomingCalls {
        item: CallHierarchyItem,
        id: u64,
    },

    CallHierarchyOutgoingCalls {
        item: CallHierarchyItem,
        id: u64,
    },

    PrepareTypeHierarchy {
        uri: Url,
        position: Position,
        id: u64,
    },

    TypeHierarchySupertypes {
        item: TypeHierarchyItem,
        id: u64,
    },

    TypeHierarchySubtypes {
        item: TypeHierarchyItem,
        id: u64,
    },

    SemanticTokensFull {
        uri: Url,
        id: u64,
    },

    SemanticTokensFullDelta {
        uri: Url,
        previous_result_id: String,
        id: u64,
    },

    SemanticTokensRange {
        uri: Url,
        range: Range,
        id: u64,
    },

    DocumentDiagnostic {
        uri: Url,
        identifier: Option<String>,
        previous_result_id: Option<String>,
        id: u64,
    },

    WorkspaceDiagnostic {
        identifier: Option<String>,
        previous_result_ids: Vec<PreviousResultId>,
        id: u64,
    },

    /// `items` must match the order of the server's `workspace/configuration` request.
    RespondConfiguration {
        id: u64,
        items: Vec<serde_json::Value>,
    },

    RespondApplyEdit {
        id: u64,
        response: ApplyWorkspaceEditResponse,
    },

    RespondShowMessageRequest {
        id: u64,
        action: Option<MessageActionItem>,
    },

    RespondShowDocument {
        id: u64,
        success: bool,
    },

    RespondWorkDoneProgressCreate {
        id: u64,
    },

    RespondRegisterCapability {
        id: u64,
    },
    RespondUnregisterCapability {
        id: u64,
    },

    RespondWorkspaceFolders {
        id: u64,
        folders: Option<Vec<WorkspaceFolder>>,
    },

    WorkDoneProgressCancel {
        token: ProgressToken,
    },

    Shutdown {
        id: u64,
    },

    Exit,
}

/// Incoming message from the language server. `*Requested` variants carry
/// `request_id` and require a matching `LspMessage::Respond*` reply.
#[derive(Debug, Clone)]
pub enum LspResponse {
    Initialized {
        capabilities: Box<ServerCapabilities>,
    },

    Diagnostics {
        uri: Url,
        /// `None` if the server omitted it (older spec). Stale versions should be discarded.
        version: Option<i32>,
        diagnostics: Vec<Diagnostic>,
    },

    LogMessage {
        typ: MessageType,
        message: String,
    },

    ShowMessage {
        typ: MessageType,
        message: String,
    },

    Progress {
        token: ProgressToken,
        value: ProgressParamsValue,
    },

    Telemetry {
        data: serde_json::Value,
    },

    LogTrace {
        message: String,
        verbose: Option<String>,
    },

    ConfigurationRequested {
        request_id: u64,
        items: Vec<ConfigurationItem>,
    },

    ApplyEditRequested {
        request_id: u64,
        label: Option<String>,
        edit: WorkspaceEdit,
    },

    ShowMessageRequestRequested {
        request_id: u64,
        typ: MessageType,
        message: String,
        actions: Option<Vec<MessageActionItem>>,
    },

    ShowDocumentRequested {
        request_id: u64,
        uri: Url,
        external: Option<bool>,
        take_focus: Option<bool>,
        selection: Option<Range>,
    },

    WorkDoneProgressCreateRequested {
        request_id: u64,
        token: ProgressToken,
    },

    RegisterCapabilityRequested {
        request_id: u64,
        registrations: Vec<Registration>,
    },

    UnregisterCapabilityRequested {
        request_id: u64,
        unregistrations: Vec<Unregistration>,
    },

    WorkspaceFoldersRequested {
        request_id: u64,
    },

    SemanticTokensRefreshRequested,
    InlayHintRefreshRequested,
    CodeLensRefreshRequested,
    DiagnosticsRefreshRequested,

    Completion {
        id: u64,
        items: Vec<CompletionItem>,
        is_incomplete: bool,
    },

    ResolvedCompletionItem {
        id: u64,
        item: Box<CompletionItem>,
    },

    Hover {
        id: u64,
        content: String,
        kind: MarkupKind,
        range: Option<Range>,
    },

    SignatureHelp {
        id: u64,
        signatures: Vec<SignatureInformation>,
        active_signature: Option<u32>,
        active_parameter: Option<u32>,
    },

    Declaration {
        id: u64,
        locations: Vec<Location>,
    },

    Definition {
        id: u64,
        locations: Vec<Location>,
    },

    TypeDefinition {
        id: u64,
        locations: Vec<Location>,
    },

    Implementation {
        id: u64,
        locations: Vec<Location>,
    },

    References {
        id: u64,
        locations: Vec<Location>,
    },

    DocumentHighlights {
        id: u64,
        highlights: Vec<DocumentHighlight>,
    },

    DocumentSymbols {
        id: u64,
        flat: Vec<SymbolInformation>,
        nested: Vec<DocumentSymbol>,
    },

    WorkspaceSymbols {
        id: u64,
        symbols: Vec<WorkspaceSymbolResponseItem>,
    },

    ResolvedWorkspaceSymbol {
        id: u64,
        symbol: WorkspaceSymbol,
    },

    FoldingRanges {
        id: u64,
        ranges: Vec<FoldingRange>,
    },

    SelectionRanges {
        id: u64,
        ranges: Vec<SelectionRange>,
    },

    CodeActions {
        id: u64,
        actions: Vec<CodeActionOrCommand>,
    },

    ResolvedCodeAction {
        id: u64,
        action: Box<CodeAction>,
    },

    Format {
        id: u64,
        edits: Vec<TextEdit>,
    },

    RangeFormatting {
        id: u64,
        edits: Vec<TextEdit>,
    },

    OnTypeFormatting {
        id: u64,
        edits: Vec<TextEdit>,
    },

    WillSaveWaitUntil {
        id: u64,
        edits: Vec<TextEdit>,
    },

    InlayHints {
        id: u64,
        hints: Vec<InlayHint>,
    },

    ResolvedInlayHint {
        id: u64,
        hint: InlayHint,
    },

    DocumentLinks {
        id: u64,
        links: Vec<DocumentLink>,
    },

    ResolvedDocumentLink {
        id: u64,
        link: DocumentLink,
    },

    DocumentColors {
        id: u64,
        colors: Vec<ColorInformation>,
    },

    ColorPresentations {
        id: u64,
        presentations: Vec<ColorPresentation>,
    },

    LinkedEditingRanges {
        id: u64,
        ranges: Option<LinkedEditingRanges>,
    },

    Monikers {
        id: u64,
        monikers: Vec<Moniker>,
    },

    PrepareRename {
        id: u64,
        range: Range,
        placeholder: Option<String>,
    },

    Rename {
        id: u64,
        edit: WorkspaceEdit,
    },

    PrepareCallHierarchy {
        id: u64,
        items: Vec<CallHierarchyItem>,
    },

    CallHierarchyIncomingCalls {
        id: u64,
        calls: Vec<CallHierarchyIncomingCall>,
    },

    CallHierarchyOutgoingCalls {
        id: u64,
        calls: Vec<CallHierarchyOutgoingCall>,
    },

    PrepareTypeHierarchy {
        id: u64,
        items: Vec<TypeHierarchyItem>,
    },

    TypeHierarchySupertypes {
        id: u64,
        items: Vec<TypeHierarchyItem>,
    },

    TypeHierarchySubtypes {
        id: u64,
        items: Vec<TypeHierarchyItem>,
    },

    SemanticTokens {
        id: u64,
        result: SemanticTokensResult,
    },

    SemanticTokensDelta {
        id: u64,
        result: SemanticTokensFullDeltaResult,
    },

    SemanticTokensRange {
        id: u64,
        result: SemanticTokensRangeResult,
    },

    DocumentDiagnostic {
        id: u64,
        report: DocumentDiagnosticReportResult,
    },

    WorkspaceDiagnostic {
        id: u64,
        report: WorkspaceDiagnosticReportResult,
    },

    ShutdownAck {
        id: u64,
    },

    Crashed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CodeActionOrCommand {
    Action(Box<CodeAction>),
    Command(lsp_types::Command),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkspaceSymbolResponseItem {
    Symbol(WorkspaceSymbol),
    Information(SymbolInformation),
}

#[derive(Message, EntityEvent, Clone, Debug)]
pub struct LspRequest {
    pub entity: Entity,
    pub msg: LspMessage,
}

macro_rules! lsp_msg {
    ($($name:ident { $($field:ident : $ty:ty),* $(,)? }),* $(,)?) => {
        $(
            #[derive(Message, Clone, Debug)]
            pub struct $name {
                pub entity: Entity,
                $(pub $field: $ty,)*
            }
        )*
    };
}

lsp_msg! {
    LspServerInitialized { capabilities: ServerCapabilities },
    LspDiagnosticsUpdated { uri: Url, version: Option<i32>, diagnostics: Vec<Diagnostic> },
    LspLogMessage { typ: MessageType, message: String },
    LspShowMessage { typ: MessageType, message: String },
    LspProgress { token: ProgressToken, value: ProgressParamsValue },
    LspTelemetry { data: serde_json::Value },
    LspLogTrace { message: String, verbose: Option<String> },

    LspConfigurationRequested { request_id: u64, items: Vec<ConfigurationItem> },
    LspApplyEditRequested { request_id: u64, label: Option<String>, edit: WorkspaceEdit },
    LspShowMessageRequestRequested {
        request_id: u64,
        typ: MessageType,
        message: String,
        actions: Option<Vec<MessageActionItem>>,
    },
    LspShowDocumentRequested {
        request_id: u64,
        uri: Url,
        external: Option<bool>,
        take_focus: Option<bool>,
        selection: Option<Range>,
    },
    LspWorkDoneProgressCreateRequested { request_id: u64, token: ProgressToken },
    LspRegisterCapabilityRequested { request_id: u64, registrations: Vec<Registration> },
    LspUnregisterCapabilityRequested { request_id: u64, unregistrations: Vec<Unregistration> },
    LspWorkspaceFoldersRequested { request_id: u64 },

    LspSemanticTokensRefreshRequested {},
    LspInlayHintRefreshRequested {},
    LspCodeLensRefreshRequested {},
    LspDiagnosticsRefreshRequested {},

    LspCompletionResponse { id: u64, items: Vec<CompletionItem>, is_incomplete: bool },
    LspResolvedCompletionItem { id: u64, item: CompletionItem },
    LspHoverResponse { id: u64, content: String, kind: MarkupKind, range: Option<Range> },
    LspSignatureHelpResponse {
        id: u64,
        signatures: Vec<SignatureInformation>,
        active_signature: Option<u32>,
        active_parameter: Option<u32>,
    },
    LspDeclarationResponse { id: u64, locations: Vec<Location> },
    LspDefinitionResponse { id: u64, locations: Vec<Location> },
    LspTypeDefinitionResponse { id: u64, locations: Vec<Location> },
    LspImplementationResponse { id: u64, locations: Vec<Location> },
    LspReferencesResponse { id: u64, locations: Vec<Location> },
    LspDocumentHighlightsResponse { id: u64, highlights: Vec<DocumentHighlight> },
    LspDocumentSymbolsResponse {
        id: u64,
        flat: Vec<SymbolInformation>,
        nested: Vec<DocumentSymbol>,
    },
    LspWorkspaceSymbolsResponse { id: u64, symbols: Vec<WorkspaceSymbolResponseItem> },
    LspResolvedWorkspaceSymbol { id: u64, symbol: WorkspaceSymbol },
    LspFoldingRangesResponse { id: u64, ranges: Vec<FoldingRange> },
    LspSelectionRangesResponse { id: u64, ranges: Vec<SelectionRange> },
    LspCodeActionsResponse { id: u64, actions: Vec<CodeActionOrCommand> },
    LspResolvedCodeAction { id: u64, action: CodeAction },
    LspFormatResponse { id: u64, edits: Vec<TextEdit> },
    LspRangeFormattingResponse { id: u64, edits: Vec<TextEdit> },
    LspOnTypeFormattingResponse { id: u64, edits: Vec<TextEdit> },
    LspWillSaveWaitUntilResponse { id: u64, edits: Vec<TextEdit> },
    LspInlayHintsResponse { id: u64, hints: Vec<InlayHint> },
    LspResolvedInlayHint { id: u64, hint: InlayHint },
    LspDocumentLinksResponse { id: u64, links: Vec<DocumentLink> },
    LspResolvedDocumentLink { id: u64, link: DocumentLink },
    LspDocumentColorsResponse { id: u64, colors: Vec<ColorInformation> },
    LspColorPresentationsResponse { id: u64, presentations: Vec<ColorPresentation> },
    LspLinkedEditingRangesResponse { id: u64, ranges: Option<LinkedEditingRanges> },
    LspMonikersResponse { id: u64, monikers: Vec<Moniker> },
    LspPrepareRenameResponse { id: u64, range: Range, placeholder: Option<String> },
    LspRenameResponse { id: u64, edit: WorkspaceEdit },
    LspPrepareCallHierarchyResponse { id: u64, items: Vec<CallHierarchyItem> },
    LspCallHierarchyIncomingCallsResponse { id: u64, calls: Vec<CallHierarchyIncomingCall> },
    LspCallHierarchyOutgoingCallsResponse { id: u64, calls: Vec<CallHierarchyOutgoingCall> },
    LspPrepareTypeHierarchyResponse { id: u64, items: Vec<TypeHierarchyItem> },
    LspTypeHierarchySupertypesResponse { id: u64, items: Vec<TypeHierarchyItem> },
    LspTypeHierarchySubtypesResponse { id: u64, items: Vec<TypeHierarchyItem> },
    LspSemanticTokensResponse { id: u64, result: SemanticTokensResult },
    LspSemanticTokensDeltaResponse { id: u64, result: SemanticTokensFullDeltaResult },
    LspSemanticTokensRangeResponse { id: u64, result: SemanticTokensRangeResult },
    LspDocumentDiagnosticResponse { id: u64, report: DocumentDiagnosticReportResult },
    LspWorkspaceDiagnosticResponse { id: u64, report: WorkspaceDiagnosticReportResult },
    LspShutdownAck { id: u64 },
    LspServerCrashed {},
}
