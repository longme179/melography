//! Token-stream `.cube` parser + validator (luts.md §1, issue #7).
//!
//! **Why token-stream, not line-based:** Adobe `.cube` tables legitimately
//! wrap across lines with any number of entries per line; a line parser
//! either rejects valid files or silently miscounts. We lex the byte stream
//! into tokens (whitespace/CRLF-tolerant, `#` comments to EOL) and run a
//! keyword/float state machine over the token stream.
//!
//! **Strictness contract (luts.md §1):** malformed files produce structured
//! errors with line numbers — never best-effort guesses. Enforced:
//! exactly one size keyword (1D or 3D, never both) · size ranges (3D 2–256,
//! 1D 2–65536) · domain bounds valid (min < max per component) · exact entry
//! counts (N³·3 resp. N·3; too few/too many are errors) · finite floats only
//! (NaN/Inf rejected) · `LUT_1D_INPUT_DURATION` rejected with an explanatory
//! error · file-size cap (64 MiB default).
//!
//! **Table layout (Adobe spec):** 3D tables are stored with the **red index
//! varying fastest** — entry `(i,j,k)` (r,g,b lattice indices) lives at
//! `table[(i + j·N + k·N²)·3..][..3]`. 1D tables have N rows × 3 output
//! columns; per-channel shaper semantics are defined in [`crate::tetra`].

use serde::{Deserialize, Serialize};

/// Default hard cap on `.cube` file size (luts.md §1; DoS guard for the
/// fuzz-facing import path). Configurable in [`ParseOptions`].
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;

/// 3D grid side-length bounds.
pub const SIZE_3D_MIN: u32 = 2;
pub const SIZE_3D_MAX: u32 = 256;
/// 1D table length bounds.
pub const SIZE_1D_MIN: u32 = 2;
pub const SIZE_1D_MAX: u32 = 65536;

/// Which table a `.cube` file declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CubeKind {
    /// `LUT_1D_SIZE N` — N rows × 3 output columns.
    Lut1d,
    /// `LUT_3D_SIZE N` — N³ lattice points × 3 outputs, red-fastest.
    Lut3d,
}

/// Non-fatal validation findings surfaced to the import dialog (UX_SPEC §6
/// LUT panel warnings; luts.md §2 "documented, not fatal").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CubeWarning {
    /// A 1D channel used as a shaper is not non-decreasing (forward use is
    /// still well-defined; inversion is not — recorded, not fatal).
    NonMonotonic1d { channel: u8, line: u32 },
    /// Domain extends outside [0,1] (HDR-ish domain; legal, worth telling
    /// the user because preview clamping semantics change).
    DomainOutsideUnit { component: u8 },
    /// 3D grid is tiny (2×2×2): legal, but interpolation quality is minimal.
    Tiny3d,
}

/// Structured parse/validation failure. `line` is 1-based; 0 = not
/// attributable to a line (e.g. file-level cap).
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[error("{kind} (line {line})")]
pub struct CubeError {
    pub line: u32,
    pub kind: CubeErrorKind,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum CubeErrorKind {
    #[error("empty file")]
    Empty,
    #[error("file exceeds size cap ({detail} bytes)")]
    TooLarge { detail: String },
    #[error("no LUT size keyword found (need LUT_1D_SIZE or LUT_3D_SIZE)")]
    MissingSize,
    #[error("both LUT_1D_SIZE and LUT_3D_SIZE present — mixed 1D+3D is invalid")]
    Mixed1d3d,
    #[error("duplicate {kw} keyword")]
    DuplicateKeyword { kw: String },
    #[error("size {n} out of range for {kind_str} (allowed {lo}–{hi})")]
    SizeOutOfRange {
        n: u64,
        kind_str: String,
        lo: u32,
        hi: u32,
    },
    #[error("DOMAIN_{bound} expects 1 or 3 floats, got {got}")]
    DomainFloatCount { bound: String, got: u32 },
    #[error("DOMAIN_{bound} value {value} is not finite")]
    DomainNotFinite { bound: String, value: f64 },
    #[error("domain invalid: min ≥ max for component {component} ({min} ≥ {max})")]
    DomainInverted { component: u8, min: f32, max: f32 },
    #[error("LUT_1D_INPUT_DURATION is not supported: this file declares a duration-remapped 1D LUT (animated/multi-duration), which MELOGRAPHY v1 does not apply — refusing rather than guessing")]
    UnsupportedKeyword { kw: String },
    #[error("unknown keyword {kw} (strict mode: no best-effort parsing)")]
    UnknownKeyword { kw: String },
    #[error("token {token:?} is not a number")]
    NotANumber { token: String },
    #[error("number {token:?} is NaN or infinite — LUT data must be finite")]
    NonFinite { token: String },
    #[error("table entry count mismatch: expected {expected} floats for a {kind_str} of size {size}, found {found}")]
    EntryCount {
        expected: usize,
        found: usize,
        kind_str: String,
        size: u32,
    },
    #[error("keyword {kw} missing its argument(s) before end of file")]
    TruncatedHeader { kw: String },
    #[error("cannot read file: {detail}")]
    IoRead { detail: String },
}

/// Tunable parse limits (defaults per luts.md §1).
#[derive(Debug, Clone, Copy)]
pub struct ParseOptions {
    pub max_bytes: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// A parsed, validated `.cube` LUT.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CubeLut {
    pub kind: CubeKind,
    /// Grid side (3D) or row count (1D).
    pub size: u32,
    /// Per-component domain lower bound.
    pub domain_min: [f32; 3],
    /// Per-component domain upper bound.
    pub domain_max: [f32; 3],
    pub title: Option<String>,
    /// 3D: `size³` entries × 3 floats, red-fastest (`(i + j·N + k·N²)·3`).
    /// 1D: `size` rows × 3 floats. Finite values only (enforced at parse).
    pub table: Vec<f32>,
    /// Non-fatal findings for the import UI.
    pub warnings: Vec<CubeWarning>,
}

impl CubeLut {
    /// Entry count (lattice points for 3D, rows for 1D).
    #[must_use]
    pub fn entries(&self) -> usize {
        match self.kind {
            CubeKind::Lut3d => (self.size as usize).pow(3),
            CubeKind::Lut1d => self.size as usize,
        }
    }

    /// Lattice point accessor for 3D tables (red-fastest layout).
    ///
    /// # Panics
    /// If `self.kind != Lut3d` or any index ≥ `size` (caller-checked paths
    /// in this crate never hit either).
    #[must_use]
    pub fn grid3(&self, i: usize, j: usize, k: usize) -> [f32; 3] {
        debug_assert!(self.kind == CubeKind::Lut3d);
        let n = self.size as usize;
        debug_assert!(i < n && j < n && k < n);
        let base = (i + j * n + k * n * n) * 3;
        [self.table[base], self.table[base + 1], self.table[base + 2]]
    }
}

/// Lexer token: a byte range in the source plus the 1-based line it starts on.
struct Token<'a> {
    text: &'a str,
    line: u32,
}

/// Lex the byte stream: UTF-8 lossy tokens separated by ASCII whitespace;
/// `#` starts a comment to end-of-line (Adobe spec). CRLF/LF/CR all count as
/// line breaks. Returns tokens *including* would-be floats; classification
/// happens in the parser.
fn lex(src: &str) -> Vec<Token<'_>> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut line = 1u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i += 2;
                } else {
                    i += 1;
                }
                line += 1;
            }
            b' ' | b'\t' | 0x0b | 0x0c => {
                i += 1;
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c | b'#')
                {
                    i += 1;
                }
                // SAFETY-free: source is &str, byte ranges are char-safe
                // because separators are ASCII.
                out.push(Token {
                    text: &src[start..i],
                    line,
                });
            }
        }
    }
    out
}

/// Parse + validate `.cube` bytes. See the module docs for the strictness
/// contract; every failure carries a line number.
pub fn parse_cube(src: &str) -> Result<CubeLut, CubeError> {
    parse_cube_with(src, ParseOptions::default())
}

/// [`parse_cube`] with explicit limits. `src` is the decoded file content
/// (callers read with lossy UTF-8: byte-level garbage in comments survives).
pub fn parse_cube_with(src: &str, opts: ParseOptions) -> Result<CubeLut, CubeError> {
    if src.len() > opts.max_bytes {
        return Err(CubeError {
            line: 0,
            kind: CubeErrorKind::TooLarge {
                detail: format!("{} > {}", src.len(), opts.max_bytes),
            },
        });
    }
    let tokens = lex(src);
    if tokens.is_empty() {
        return Err(CubeError {
            line: 0,
            kind: CubeErrorKind::Empty,
        });
    }

    let mut kind: Option<CubeKind> = None;
    let mut size: Option<u32> = None;
    let mut domain_min: Option<[f32; 3]> = None;
    let mut domain_max: Option<[f32; 3]> = None;
    let mut title: Option<String> = None;
    let mut table: Vec<f32> = Vec::new();
    // Line of the first table float, for entry-count error attribution.
    let mut first_float_line: Option<u32> = None;

    // ---- pass 1: classify every token (keywords consume args; TITLE
    // consumes the rest of its line; everything else must be a float). The
    // lexer dropped line structure for comments but TITLE needs "rest of
    // line" — handle by slicing the source at the token start.
    let mut ti = 0usize;
    while ti < tokens.len() {
        let tok = &tokens[ti];
        let t = tok.text;
        let up = t.to_ascii_uppercase();
        match up.as_str() {
            "LUT_1D_SIZE" | "LUT_3D_SIZE" => {
                let this_kind = if up.starts_with("LUT_1D") {
                    CubeKind::Lut1d
                } else {
                    CubeKind::Lut3d
                };
                if kind.is_some() {
                    if kind == Some(this_kind) {
                        return Err(CubeError {
                            line: tok.line,
                            kind: CubeErrorKind::DuplicateKeyword { kw: up.clone() },
                        });
                    }
                    return Err(CubeError {
                        line: tok.line,
                        kind: CubeErrorKind::Mixed1d3d,
                    });
                }
                let arg = tokens.get(ti + 1).ok_or_else(|| CubeError {
                    line: tok.line,
                    kind: CubeErrorKind::TruncatedHeader { kw: up.clone() },
                })?;
                let n: u64 = arg.text.parse().map_err(|_| CubeError {
                    line: arg.line,
                    kind: CubeErrorKind::NotANumber {
                        token: arg.text.to_string(),
                    },
                })?;
                let (lo, hi) = match this_kind {
                    CubeKind::Lut1d => (SIZE_1D_MIN, SIZE_1D_MAX),
                    CubeKind::Lut3d => (SIZE_3D_MIN, SIZE_3D_MAX),
                };
                if n < lo as u64 || n > hi as u64 {
                    return Err(CubeError {
                        line: arg.line,
                        kind: CubeErrorKind::SizeOutOfRange {
                            n,
                            kind_str: format!("{this_kind:?}"),
                            lo,
                            hi,
                        },
                    });
                }
                kind = Some(this_kind);
                size = Some(n as u32);
                ti += 2;
            }
            "DOMAIN_MIN" | "DOMAIN_MAX" => {
                let is_min = up == "DOMAIN_MIN";
                // 1 or 3 floats (luts.md §1: scalar form covers the "2
                // floats across the pair" variant; 3+3 is the standard).
                // Args live on the SAME LINE as the keyword (Adobe grammar:
                // domain headers are single-line) — this stops a domain
                // keyword from eating the first table floats of the next
                // line (bug caught by the parser test suite).
                let mut vals = [0f32; 3];
                let mut got = 0u32;
                while got < 3 {
                    let Some(arg) = tokens.get(ti + 1 + got as usize) else {
                        break;
                    };
                    if arg.line != tok.line {
                        break;
                    }
                    let Ok(v) = arg.text.parse::<f32>() else {
                        break;
                    };
                    if !v.is_finite() {
                        return Err(CubeError {
                            line: arg.line,
                            kind: CubeErrorKind::DomainNotFinite {
                                bound: if is_min { "MIN" } else { "MAX" }.into(),
                                value: v as f64,
                            },
                        });
                    }
                    vals[got as usize] = v;
                    got += 1;
                }
                if got == 0 {
                    return Err(CubeError {
                        line: tok.line,
                        kind: CubeErrorKind::TruncatedHeader { kw: up.clone() },
                    });
                }
                match got {
                    1 => {
                        let v = vals[0];
                        vals.fill(v);
                    }
                    3 => {}
                    n => {
                        return Err(CubeError {
                            line: tok.line,
                            kind: CubeErrorKind::DomainFloatCount {
                                bound: if is_min { "MIN" } else { "MAX" }.into(),
                                got: n,
                            },
                        });
                    }
                }
                if is_min {
                    if domain_min.is_some() {
                        return Err(CubeError {
                            line: tok.line,
                            kind: CubeErrorKind::DuplicateKeyword { kw: up.clone() },
                        });
                    }
                    domain_min = Some(vals);
                } else {
                    if domain_max.is_some() {
                        return Err(CubeError {
                            line: tok.line,
                            kind: CubeErrorKind::DuplicateKeyword { kw: up.clone() },
                        });
                    }
                    domain_max = Some(vals);
                }
                ti += 1 + got as usize;
            }
            "TITLE" => {
                if title.is_some() {
                    return Err(CubeError {
                        line: tok.line,
                        kind: CubeErrorKind::DuplicateKeyword { kw: "TITLE".into() },
                    });
                }
                // Rest of the physical line after the keyword (Adobe spec).
                // Find the token's offset in src, then cut at the next \n.
                let offset = tok.text.as_ptr() as usize - src.as_ptr() as usize + "TITLE".len();
                let rest = &src[offset..];
                let line_end = rest.find('\n').unwrap_or(rest.len());
                let raw = rest[..line_end].trim_end_matches('\r').trim();
                // Adobe grammar: TITLE "quoted string" — strip one pair of
                // surrounding double quotes when present (unquoted titles
                // are tolerated; embedded quotes stay literal).
                let unquoted = raw
                    .strip_prefix('"')
                    .and_then(|t| t.strip_suffix('"'))
                    .unwrap_or(raw);
                title = Some(unquoted.to_string());
                // Skip every token that starts on this same line.
                let this_line = tok.line;
                ti += 1;
                while ti < tokens.len() && tokens[ti].line == this_line {
                    ti += 1;
                }
            }
            "LUT_1D_INPUT_DURATION" => {
                return Err(CubeError {
                    line: tok.line,
                    kind: CubeErrorKind::UnsupportedKeyword { kw: up.clone() },
                });
            }
            _ if looks_like_keyword(t) => {
                // Known-header-only grammar: an ALL-CAPS leading-alpha token
                // that fails to parse as a number is a keyword we don't know.
                return Err(CubeError {
                    line: tok.line,
                    kind: CubeErrorKind::UnknownKeyword { kw: t.to_string() },
                });
            }
            _ => {
                let v: f32 = t.parse().map_err(|_| CubeError {
                    line: tok.line,
                    kind: CubeErrorKind::NotANumber {
                        token: t.to_string(),
                    },
                })?;
                if !v.is_finite() {
                    return Err(CubeError {
                        line: tok.line,
                        kind: CubeErrorKind::NonFinite {
                            token: t.to_string(),
                        },
                    });
                }
                if first_float_line.is_none() {
                    first_float_line = Some(tok.line);
                }
                table.push(v);
                ti += 1;
            }
        }
    }

    // ---- pass 2: cross-field validation.
    let kind = kind.ok_or(CubeError {
        line: 0,
        kind: CubeErrorKind::MissingSize,
    })?;
    let size = size.unwrap();
    let domain_min = domain_min.unwrap_or([0.0, 0.0, 0.0]);
    let domain_max = domain_max.unwrap_or([1.0, 1.0, 1.0]);
    for c in 0..3 {
        if domain_min[c] >= domain_max[c] {
            return Err(CubeError {
                line: first_float_line.unwrap_or(1),
                kind: CubeErrorKind::DomainInverted {
                    component: c as u8,
                    min: domain_min[c],
                    max: domain_max[c],
                },
            });
        }
    }
    let expected = match kind {
        CubeKind::Lut3d => (size as usize).pow(3) * 3,
        CubeKind::Lut1d => size as usize * 3,
    };
    if table.len() != expected {
        return Err(CubeError {
            line: first_float_line.unwrap_or(1),
            kind: CubeErrorKind::EntryCount {
                expected,
                found: table.len(),
                kind_str: format!("{kind:?}"),
                size,
            },
        });
    }

    // ---- pass 3: non-fatal warnings.
    let mut warnings = Vec::new();
    if kind == CubeKind::Lut1d {
        for ch in 0..3usize {
            let col: Vec<f32> = (0..size as usize).map(|r| table[r * 3 + ch]).collect();
            if col.windows(2).any(|w| w[0] > w[1]) {
                warnings.push(CubeWarning::NonMonotonic1d {
                    channel: ch as u8,
                    line: first_float_line.unwrap_or(1),
                });
            }
        }
    }
    if kind == CubeKind::Lut3d {
        if size <= 2 {
            warnings.push(CubeWarning::Tiny3d);
        }
        for c in 0..3 {
            if domain_min[c] < 0.0 || domain_max[c] > 1.0 {
                warnings.push(CubeWarning::DomainOutsideUnit { component: c as u8 });
                break;
            }
        }
    }

    Ok(CubeLut {
        kind,
        size,
        domain_min,
        domain_max,
        title,
        table,
        warnings,
    })
}

/// A token "looks like a keyword" when it starts ASCII-alphabetic and
/// contains no `.`/`e`-style numeric forms — i.e. it can never parse as a
/// float. Used to reject unknown keywords instead of letting them fall
/// through as garbage-number errors (better diagnostics, strict grammar).
fn looks_like_keyword(t: &str) -> bool {
    let mut chars = t.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    // Floats can start with a letter only via inf/nan — which are rejected
    // separately as NonFinite after successful parse.
    let lower = t.to_ascii_lowercase();
    if lower == "inf"
        || lower == "infinity"
        || lower == "nan"
        || lower == "-inf"
        || lower == "-infinity"
        || lower == "-nan"
        || lower == "+inf"
        || lower == "+nan"
    {
        return false; // let the float path produce the precise error
    }
    true
}

/// Convenience: read + parse a `.cube` file (strict size cap, lossy UTF-8 —
/// byte garbage may only survive inside comments/titles, never in data).
pub fn parse_cube_file(path: &std::path::Path) -> Result<CubeLut, CubeError> {
    let bytes = std::fs::read(path).map_err(|e| CubeError {
        line: 0,
        kind: CubeErrorKind::IoRead {
            detail: format!("{}: {e}", path.display()),
        },
    })?;
    let src = String::from_utf8_lossy(&bytes);
    parse_cube(&src)
}
