//! LSP snippet expansion.
//!
//! Turns LSP `insertText` strings such as `println!($1)` or
//! `for ${1:item} in ${2:iter} { $0 }` into:
//!   - the plain text the editor should actually insert, and
//!   - a list of tabstop ranges (in char offsets within the inserted text)
//!     that the tabstop session walks Tab-by-Tab.
//!
//! Subset supported (matches what rust-analyzer / typescript-language-server
//! produce in practice):
//!   - `$N` simple tabstop
//!   - `${N}` braced tabstop
//!   - `${N:placeholder}` tabstop with placeholder text
//!   - `$0` final stop (cursor lands here on session end)
//!   - `\$`, `\}`, `\\` escapes
//!
//! Out of scope (not produced by the servers we target): regex transforms
//! (`${1:/upcase}`), choice (`${1|a,b,c|}`), variables (`$TM_FILENAME`).
//! These are lexed but treated as plain text so the snippet still inserts
//! something sane.

/// One tabstop range, expressed as character offsets *within* the inserted
/// snippet body. The editor adds the insertion's start char index to each
/// endpoint to get an absolute rope position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tabstop {
    /// LSP tabstop id (`$1`, `$2`, …). `0` means the final stop.
    pub id: u32,
    /// `[start..end)` char range (within the inserted body) that the
    /// session selects when this tabstop is active. End equals start
    /// when the tabstop has no placeholder.
    pub start: usize,
    pub end: usize,
}

/// Parsed snippet: the literal text to insert plus its tabstops.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedSnippet {
    /// The text to insert into the rope.
    pub text: String,
    /// Tabstops in declaration order. The session sorts by `id` to walk
    /// them Tab-by-Tab; `id == 0` is the final stop.
    pub tabstops: Vec<Tabstop>,
}

impl ParsedSnippet {
    /// Returns `true` when there are no tabstops — the caller can then
    /// skip starting a session and just insert `text` verbatim.
    pub fn has_tabstops(&self) -> bool {
        !self.tabstops.is_empty()
    }
}

/// Parse an LSP snippet body. Always succeeds — unrecognized constructs
/// are passed through as plain text.
pub fn parse(body: &str) -> ParsedSnippet {
    let mut text = String::with_capacity(body.len());
    let mut tabstops = Vec::new();
    let mut chars = body.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // Recognised escapes: `\$`, `\}`, `\\`. Anything else
                // passes through with the backslash preserved.
                match chars.peek() {
                    Some('$') | Some('}') | Some('\\') => {
                        text.push(chars.next().unwrap());
                    }
                    _ => text.push('\\'),
                }
            }
            '$' => {
                if let Some(&next) = chars.peek() {
                    if next == '{' {
                        chars.next();
                        if let Some(stop) = parse_braced(&mut chars, &mut text) {
                            tabstops.push(stop);
                        }
                    } else if next.is_ascii_digit() {
                        let id = read_digits(&mut chars);
                        let start = text.chars().count();
                        tabstops.push(Tabstop {
                            id,
                            start,
                            end: start,
                        });
                    } else {
                        text.push('$');
                    }
                } else {
                    text.push('$');
                }
            }
            other => text.push(other),
        }
    }

    ParsedSnippet { text, tabstops }
}

/// Read a run of ASCII digits as `u32`. The caller has already peeked at
/// the first digit.
fn read_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u32 {
    let mut n: u32 = 0;
    while let Some(&c) = chars.peek() {
        if let Some(d) = c.to_digit(10) {
            n = n.saturating_mul(10).saturating_add(d);
            chars.next();
        } else {
            break;
        }
    }
    n
}

/// Parse `${N}` or `${N:placeholder}`. The opening `{` has been consumed.
/// On any unknown shape (`${1|…|}`, `${1/…/…/}`, `$TM_FILENAME`) we drop
/// back to literal text and skip to the matching `}`.
fn parse_braced(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    text: &mut String,
) -> Option<Tabstop> {
    let &first = chars.peek()?;
    if !first.is_ascii_digit() {
        // Variable substitution like `${TM_FILENAME}` — skip until `}`.
        skip_until_brace(chars);
        return None;
    }

    let id = read_digits(chars);
    let start = text.chars().count();

    match chars.peek() {
        Some('}') => {
            chars.next();
            Some(Tabstop {
                id,
                start,
                end: start,
            })
        }
        Some(':') => {
            chars.next();
            // Placeholder content. May itself contain `$N` references —
            // we don't recurse; just take everything up to the matching
            // `}` as literal placeholder text. This keeps the ranges
            // simple and matches what most editors render.
            while let Some(c) = chars.next() {
                match c {
                    '\\' => match chars.peek() {
                        Some('$') | Some('}') | Some('\\') => {
                            text.push(chars.next().unwrap());
                        }
                        _ => text.push('\\'),
                    },
                    '}' => {
                        let end = text.chars().count();
                        return Some(Tabstop { id, start, end });
                    }
                    other => text.push(other),
                }
            }
            // Unterminated placeholder: include the empty range anyway.
            let end = text.chars().count();
            Some(Tabstop { id, start, end })
        }
        Some('|') | Some('/') => {
            // Choice / transform — not supported. Skip to `}` and emit
            // a zero-width tabstop so Tab still has somewhere to land.
            skip_until_brace(chars);
            Some(Tabstop {
                id,
                start,
                end: start,
            })
        }
        _ => {
            skip_until_brace(chars);
            Some(Tabstop {
                id,
                start,
                end: start,
            })
        }
    }
}

fn skip_until_brace(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let mut depth = 1;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_no_tabstops() {
        let p = parse("hello world");
        assert_eq!(p.text, "hello world");
        assert!(p.tabstops.is_empty());
    }

    #[test]
    fn simple_dollar_tabstop() {
        let p = parse("println!($1)");
        assert_eq!(p.text, "println!()");
        assert_eq!(
            p.tabstops,
            vec![Tabstop {
                id: 1,
                start: 9,
                end: 9
            }]
        );
    }

    #[test]
    fn final_zero_stop() {
        let p = parse("if true { $0 }");
        assert_eq!(p.text, "if true {  }");
        assert_eq!(
            p.tabstops,
            vec![Tabstop {
                id: 0,
                start: 10,
                end: 10
            }]
        );
    }

    #[test]
    fn braced_with_placeholder() {
        let p = parse("for ${1:item} in iter {}");
        assert_eq!(p.text, "for item in iter {}");
        assert_eq!(
            p.tabstops,
            vec![Tabstop {
                id: 1,
                start: 4,
                end: 8
            }]
        );
    }

    #[test]
    fn multiple_tabstops() {
        let p = parse("${1:a} + ${2:b}");
        assert_eq!(p.text, "a + b");
        assert_eq!(
            p.tabstops,
            vec![
                Tabstop {
                    id: 1,
                    start: 0,
                    end: 1
                },
                Tabstop {
                    id: 2,
                    start: 4,
                    end: 5
                },
            ]
        );
    }

    #[test]
    fn escape_dollar() {
        let p = parse("price\\$5");
        assert_eq!(p.text, "price$5");
        assert!(p.tabstops.is_empty());
    }

    #[test]
    fn unsupported_choice_collapses_to_zero_width() {
        let p = parse("${1|red,green,blue|}");
        // Placeholder skipped, but the tabstop is still there.
        assert_eq!(p.text, "");
        assert_eq!(
            p.tabstops,
            vec![Tabstop {
                id: 1,
                start: 0,
                end: 0
            }]
        );
    }

    #[test]
    fn unicode_safe_offsets() {
        let p = parse("café ${1:τ}");
        assert_eq!(p.text, "café τ");
        // 'café ' is 5 chars
        assert_eq!(
            p.tabstops,
            vec![Tabstop {
                id: 1,
                start: 5,
                end: 6
            }]
        );
    }
}
