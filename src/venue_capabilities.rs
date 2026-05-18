//! Venue capabilities (typed extraction of per-venue differences).
//!
//! Phase 8 §3.5: skeleton with hardcoded values. Phase 9 will replace
//! `for_venue` with extraction from `Ready.capabilities.venue_capabilities`
//! once the Python side wires it in.
//!
//! Centralising venue branching here prevents per-venue `if venue == "kabu"`
//! checks from leaking across the Rust UI codebase.

/// Per-venue capability flags. All fields are sourced from the Phase 8
/// venue audits (kabu skill ADR S4 / R7, Tachibana ADR). See plan §3.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VenueCapabilities {
    /// Tachibana=true (PATCH /order supported) / kabu=false (cancel+resend only, kabu skill R7).
    pub supports_order_correction: bool,
    /// kabu=50 (REGIST cap) / Tachibana=u32::MAX as "unspecified".
    pub max_subscribed_instruments: u32,
    /// Tachibana=true (2nd password for orders) / kabu=false.
    pub requires_second_password: bool,
    /// Tachibana=true (token survives restart) / kabu=false (kabu skill ADR S4: must re-login).
    pub token_persists_across_restart: bool,
}

/// Resolve hardcoded capabilities for a known venue id.
///
/// Returns `None` for unknown venues; callers should treat unknown venues
/// as "no capabilities asserted" rather than defaulting silently.
///
/// Phase 9 TODO: replace body with lookup into `Ready.capabilities`.
pub fn for_venue(venue_id: &str) -> Option<VenueCapabilities> {
    match venue_id {
        "tachibana" => Some(VenueCapabilities {
            supports_order_correction: true,
            max_subscribed_instruments: u32::MAX,
            requires_second_password: true,
            token_persists_across_restart: true,
        }),
        "kabu" => Some(VenueCapabilities {
            supports_order_correction: false,
            max_subscribed_instruments: 50,
            requires_second_password: false,
            token_persists_across_restart: false,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tachibana_capabilities_match_plan() {
        let c = for_venue("tachibana").expect("tachibana must be known");
        assert!(c.supports_order_correction, "Tachibana supports PATCH /order");
        assert!(c.requires_second_password, "Tachibana requires 2nd password");
        assert!(c.token_persists_across_restart, "Tachibana token survives restart");
        // max_subscribed_instruments: plan says "unspecified"; we encode as u32::MAX.
        assert_eq!(c.max_subscribed_instruments, u32::MAX);
    }

    #[test]
    fn kabu_capabilities_match_plan() {
        let c = for_venue("kabu").expect("kabu must be known");
        assert!(!c.supports_order_correction, "kabu has no PATCH; cancel+resend only (R7)");
        assert!(!c.requires_second_password);
        assert!(!c.token_persists_across_restart, "kabu must re-login per ADR S4");
        assert_eq!(c.max_subscribed_instruments, 50, "kabu REGIST cap");
    }

    #[test]
    fn unknown_venue_returns_none() {
        assert!(for_venue("bitmex").is_none());
        assert!(for_venue("").is_none());
    }

    #[test]
    fn capabilities_are_copy() {
        // Smoke: VenueCapabilities is Copy so UI code can pass it around freely.
        let c = for_venue("kabu").unwrap();
        let c2 = c;
        assert_eq!(c, c2);
    }
}
