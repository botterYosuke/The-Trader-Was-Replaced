//! Per-entity LSP server capabilities. All `supports_*` predicates return
//! `false` until [`LspResponse::Initialized`](crate::LspResponse::Initialized)
//! has been observed.

use bevy_ecs::prelude::*;
use lsp_types::*;

#[derive(Component, Debug, Default, Clone)]
pub struct ServerCapabilities {
    inner: Option<lsp_types::ServerCapabilities>,
}

impl ServerCapabilities {
    pub fn new() -> Self {
        Self { inner: None }
    }

    pub fn set(&mut self, capabilities: lsp_types::ServerCapabilities) {
        self.inner = Some(capabilities);
    }

    pub fn get(&self) -> Option<&lsp_types::ServerCapabilities> {
        self.inner.as_ref()
    }

    pub fn supports_completion(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.completion_provider.is_some())
    }

    /// Falls back to `false` when the server omits `completionProvider.resolveProvider`.
    pub fn supports_completion_resolve(&self) -> bool {
        self.inner
            .as_ref()
            .and_then(|c| c.completion_provider.as_ref())
            .and_then(|p| p.resolve_provider)
            .unwrap_or(false)
    }

    pub fn supports_hover(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.hover_provider {
                Some(HoverProviderCapability::Simple(b)) => *b,
                Some(HoverProviderCapability::Options(_)) => true,
                None => false,
            })
    }

    pub fn supports_definition(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.definition_provider {
                Some(OneOf::Left(b)) => *b,
                Some(OneOf::Right(_)) => true,
                None => false,
            })
    }

    pub fn supports_references(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.references_provider {
                Some(OneOf::Left(b)) => *b,
                Some(OneOf::Right(_)) => true,
                None => false,
            })
    }

    pub fn supports_formatting(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.document_formatting_provider {
                Some(OneOf::Left(b)) => *b,
                Some(OneOf::Right(_)) => true,
                None => false,
            })
    }

    pub fn supports_signature_help(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.signature_help_provider.is_some())
    }

    pub fn supports_code_actions(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.code_action_provider {
                Some(CodeActionProviderCapability::Simple(b)) => *b,
                Some(CodeActionProviderCapability::Options(_)) => true,
                None => false,
            })
    }

    pub fn supports_inlay_hints(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.inlay_hint_provider {
                Some(OneOf::Left(b)) => *b,
                Some(OneOf::Right(_)) => true,
                None => false,
            })
    }

    pub fn signature_help_triggers(&self) -> Vec<String> {
        self.inner
            .as_ref()
            .and_then(|c| {
                c.signature_help_provider
                    .as_ref()
                    .and_then(|p| p.trigger_characters.clone())
            })
            .unwrap_or_default()
    }

    /// Negotiated per LSP 3.17+. Spec default (omitted field) is UTF-16.
    pub fn position_encoding(&self) -> crate::pos::PositionEncoding {
        use lsp_types::PositionEncodingKind;
        let raw = self
            .inner
            .as_ref()
            .and_then(|c| c.position_encoding.clone());
        match raw {
            Some(k) if k == PositionEncodingKind::UTF8 => crate::pos::PositionEncoding::Utf8,
            Some(k) if k == PositionEncodingKind::UTF32 => crate::pos::PositionEncoding::Utf32,
            _ => crate::pos::PositionEncoding::Utf16,
        }
    }

    pub fn completion_triggers(&self) -> Vec<String> {
        self.inner
            .as_ref()
            .and_then(|c| {
                c.completion_provider
                    .as_ref()
                    .and_then(|p| p.trigger_characters.clone())
            })
            .unwrap_or_default()
    }

    pub fn supports_document_highlight(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.document_highlight_provider {
                Some(OneOf::Left(b)) => *b,
                Some(OneOf::Right(_)) => true,
                None => false,
            })
    }

    pub fn supports_rename(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.rename_provider {
                Some(OneOf::Left(b)) => *b,
                Some(OneOf::Right(_)) => true,
                None => false,
            })
    }

    pub fn supports_prepare_rename(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.rename_provider {
                Some(OneOf::Right(opts)) => opts.prepare_provider.unwrap_or(false),
                _ => false,
            })
    }

    pub fn supports_declaration(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.declaration_provider.is_some())
    }

    pub fn supports_type_definition(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.type_definition_provider.is_some())
    }

    pub fn supports_implementation(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.implementation_provider.is_some())
    }

    pub fn supports_document_symbol(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.document_symbol_provider {
                Some(OneOf::Left(b)) => *b,
                Some(OneOf::Right(_)) => true,
                None => false,
            })
    }

    pub fn supports_workspace_symbol(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.workspace_symbol_provider.is_some())
    }

    pub fn supports_folding_range(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.folding_range_provider.is_some())
    }

    pub fn supports_selection_range(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.selection_range_provider.is_some())
    }

    pub fn supports_range_formatting(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| match &c.document_range_formatting_provider {
                Some(OneOf::Left(b)) => *b,
                Some(OneOf::Right(_)) => true,
                None => false,
            })
    }

    pub fn supports_on_type_formatting(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.document_on_type_formatting_provider.is_some())
    }

    pub fn supports_document_link(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.document_link_provider.is_some())
    }

    pub fn supports_document_color(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.color_provider.is_some())
    }

    pub fn supports_linked_editing_range(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.linked_editing_range_provider.is_some())
    }

    pub fn supports_moniker(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.moniker_provider.is_some())
    }

    pub fn supports_call_hierarchy(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.call_hierarchy_provider.is_some())
    }

    pub fn supports_semantic_tokens(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.semantic_tokens_provider.is_some())
    }

    pub fn supports_pull_diagnostics(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|c| c.diagnostic_provider.is_some())
    }

    pub fn supports_code_action_resolve(&self) -> bool {
        self.inner
            .as_ref()
            .and_then(|c| c.code_action_provider.as_ref())
            .and_then(|p| match p {
                CodeActionProviderCapability::Options(opts) => opts.resolve_provider,
                _ => None,
            })
            .unwrap_or(false)
    }

    pub fn supports_inlay_hint_resolve(&self) -> bool {
        self.inner
            .as_ref()
            .and_then(|c| match c.inlay_hint_provider.as_ref()? {
                OneOf::Right(InlayHintServerCapabilities::Options(opts)) => opts.resolve_provider,
                OneOf::Right(InlayHintServerCapabilities::RegistrationOptions(opts)) => {
                    opts.inlay_hint_options.resolve_provider
                }
                _ => None,
            })
            .unwrap_or(false)
    }
}
