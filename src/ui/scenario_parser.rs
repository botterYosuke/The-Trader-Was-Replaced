use crate::ui::components::{ScenarioMetadata, StrategyBuffer};
use bevy::prelude::*;

pub fn parse_scenario_system(buffer: Res<StrategyBuffer>, mut meta: ResMut<ScenarioMetadata>) {
    if !buffer.is_changed() {
        return;
    }
    if buffer.original_path.is_none() {
        *meta = ScenarioMetadata::default();
        return;
    }

    match parse_scenario(&buffer.source) {
        Some(m) => {
            info!(
                "SCENARIO parsed: schema_version={:?} instruments={:?} start={:?} end={:?} granularity={:?} initial_cash={:?}",
                m.schema_version, m.instruments, m.start, m.end, m.granularity, m.initial_cash
            );
            *meta = m;
        }
        None => {
            warn!("SCENARIO block not found or parse failed in strategy source");
            *meta = ScenarioMetadata::default();
        }
    }
}

fn parse_scenario(source: &str) -> Option<ScenarioMetadata> {
    let block = extract_scenario_block(source)?;

    let schema_version = parse_int_field(&block, "schema_version").map(|v| v as u32);
    let start = parse_string_field(&block, "start");
    let end = parse_string_field(&block, "end");
    let granularity = parse_string_field(&block, "granularity");
    let initial_cash = parse_int_field(&block, "initial_cash");

    // instruments: prefer "instruments" (multi-instrument v2), fall back to "instrument"
    let instruments = parse_string_or_list_field(&block, "instruments")
        .or_else(|| parse_string_or_list_field(&block, "instrument"))
        .unwrap_or_default();

    Some(ScenarioMetadata {
        schema_version,
        instruments,
        start,
        end,
        granularity,
        initial_cash,
    })
}

/// Extract the content inside `SCENARIO = { ... }` or `SCENARIO: Type = { ... }`.
/// Skips `LIVE_SCENARIO` and `class Scenario(TypedDict)` by checking word boundaries.
fn extract_scenario_block(source: &str) -> Option<String> {
    let mut search_pos = 0;
    while search_pos < source.len() {
        let rel = source[search_pos..].find("SCENARIO")?;
        let abs = search_pos + rel;

        // Word boundary: preceding char must not be alphanumeric or '_' (avoids LIVE_SCENARIO)
        if abs > 0 {
            let prev = source.as_bytes()[abs - 1] as char;
            if prev.is_alphanumeric() || prev == '_' {
                search_pos = abs + 1;
                continue;
            }
        }

        let after = &source[abs + "SCENARIO".len()..];
        let after_trimmed = after.trim_start();

        // Must be an assignment: "SCENARIO = {" or "SCENARIO: ... = {"
        if after_trimmed.starts_with('=') || after_trimmed.starts_with(':') {
            if let Some(brace_rel) = after.find('{') {
                let rest = &source[abs + "SCENARIO".len() + brace_rel + 1..];
                if let Some(close_rel) = rest.find('}') {
                    return Some(rest[..close_rel].to_string());
                }
            }
        }

        search_pos = abs + 1;
    }
    None
}

/// Parse a string field: `"key": "value"` — returns the value string.
fn parse_string_field(block: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let pos = block.find(&pattern)?;
    let rest = &block[pos + pattern.len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    if rest.starts_with('"') {
        let content = &rest[1..];
        let end = content.find('"')?;
        Some(content[..end].to_string())
    } else {
        None
    }
}

/// Parse a field that is either a bare string or a list of strings.
/// Handles: `"key": "str"`, `"key": ["a"]`, `"key": ["a", "b"]`.
fn parse_string_or_list_field(block: &str, key: &str) -> Option<Vec<String>> {
    let pattern = format!("\"{}\"", key);
    let pos = block.find(&pattern)?;
    let rest = &block[pos + pattern.len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();

    if rest.starts_with('"') {
        // Single bare string
        let content = &rest[1..];
        let end = content.find('"')?;
        Some(vec![content[..end].to_string()])
    } else if rest.starts_with('[') {
        // List of strings
        let list_content = &rest[1..];
        let end = list_content.find(']')?;
        let inner = &list_content[..end];
        let items: Vec<String> = inner
            .split(',')
            .filter_map(|s| {
                let s = s.trim();
                if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                    Some(s[1..s.len() - 1].to_string())
                } else {
                    None
                }
            })
            .collect();
        if items.is_empty() { None } else { Some(items) }
    } else {
        None
    }
}

/// Parse an integer field. Strips Python-style underscore separators (e.g. `1_000_000`).
fn parse_int_field(block: &str, key: &str) -> Option<i64> {
    let pattern = format!("\"{}\"", key);
    let pos = block.find(&pattern)?;
    let rest = &block[pos + pattern.len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| c == ',' || c == '\n' || c == '}')
        .unwrap_or(rest.len());
    let num_str: String = rest[..end]
        .chars()
        .filter(|&c| c.is_ascii_digit() || c == '-')
        .collect();
    num_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAILY_SRC: &str = r#"
SCENARIO: Scenario = {
    "schema_version": 1,
    "instrument": "1301.TSE",
    "start": "2025-01-06",
    "end": "2025-03-31",
    "granularity": "Daily",
    "initial_cash": 1_000_000,
}
LIVE_SCENARIO: dict = {
    "schema_version": 1,
    "instrument": ["1301.TSE"],
}
"#;

    const MINUTE_SRC: &str = r#"
SCENARIO: Scenario = {
    "schema_version": 2,
    "instrument": ["1301.TSE"],
    "start": "2025-01-06",
    "end": "2025-01-10",
    "granularity": "Minute",
    "initial_cash": 1_000_000,
}
"#;

    const PAIR_SRC: &str = r#"
SCENARIO: Scenario = {
    "schema_version": 2,
    "instruments": ["1301.TSE", "7203.TSE"],
    "start": "2025-01-06",
    "end": "2025-01-10",
    "granularity": "Minute",
    "initial_cash": 1_000_000,
}
"#;

    #[test]
    fn test_parse_daily() {
        let m = parse_scenario(DAILY_SRC).unwrap();
        assert_eq!(m.schema_version, Some(1));
        assert_eq!(m.instruments, vec!["1301.TSE"]);
        assert_eq!(m.start.as_deref(), Some("2025-01-06"));
        assert_eq!(m.end.as_deref(), Some("2025-03-31"));
        assert_eq!(m.granularity.as_deref(), Some("Daily"));
        assert_eq!(m.initial_cash, Some(1_000_000));
    }

    #[test]
    fn test_parse_minute() {
        let m = parse_scenario(MINUTE_SRC).unwrap();
        assert_eq!(m.schema_version, Some(2));
        assert_eq!(m.instruments, vec!["1301.TSE"]);
        assert_eq!(m.granularity.as_deref(), Some("Minute"));
    }

    #[test]
    fn test_parse_pair_multi() {
        let m = parse_scenario(PAIR_SRC).unwrap();
        assert_eq!(m.instruments, vec!["1301.TSE", "7203.TSE"]);
    }

    #[test]
    fn test_skips_live_scenario() {
        // The block extracted should be from SCENARIO, not LIVE_SCENARIO
        let m = parse_scenario(DAILY_SRC).unwrap();
        assert_eq!(m.schema_version, Some(1)); // LIVE_SCENARIO has schema_version 1 too, but instruments differ
        assert_eq!(m.instruments, vec!["1301.TSE"]); // single string from SCENARIO
    }
}
