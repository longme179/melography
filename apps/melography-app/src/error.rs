//! UI-facing error model (ARCHITECTURE §8 taxonomy → stable codes the UI
//! catalogs localize; every error carries an actionable message and never
//! a bare string). Codes are part of the UI contract: the EN/VI catalogs
//! key on them — add new codes additively, never rename.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    /// Stable dot-code (e.g. `lut.missing`) — the i18n catalog key.
    pub code: String,
    /// Developer-readable message (always shown as secondary detail).
    pub message: String,
    /// Optional file/hash context for "locate the file" flows.
    pub context: Option<String>,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            context: None,
        }
    }
    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(c) = &self.context {
            write!(f, " ({c})")?;
        }
        Ok(())
    }
}

impl From<melography_lut::LutError> for AppError {
    fn from(e: melography_lut::LutError) -> Self {
        use melography_lut::LutError as E;
        match e {
            E::Missing { hash } => AppError::new(
                "lut.missing",
                format!("LUT {hash} is missing from the library"),
            )
            .with_context(hash),
            E::ChecksumMismatch { hash } => AppError::new(
                "lut.checksum",
                format!("LUT {hash} failed its integrity check"),
            )
            .with_context(hash),
            E::EncodingNotImplemented(enc) => AppError::new(
                "lut.encoding_unimplemented",
                format!("encoding {enc} is not implemented in this build"),
            ),
            E::EncodingUnknown(enc) => AppError::new(
                "lut.encoding_unknown",
                format!("unknown encoding identifier {enc:?}"),
            ),
            E::EncodingMismatch { expected, provided } => AppError::new(
                "lut.encoding_mismatch",
                format!("LUT expects {expected}, pipeline provides {provided}"),
            ),
            E::Malformed { detail } => {
                AppError::new("lut.malformed", format!("malformed LUT: {detail}"))
            }
            E::MalformedFile { path, source } => {
                AppError::new("lut.malformed", format!("{}: {source}", path)).with_context(path)
            }
            E::Unsupported(detail) => {
                AppError::new("lut.unsupported", format!("unsupported LUT: {detail}"))
            }
            E::LibraryIo { detail } => {
                AppError::new("lut.library_io", format!("library I/O: {detail}"))
            }
        }
    }
}

impl From<melography_lut::LibraryError> for AppError {
    fn from(e: melography_lut::LibraryError) -> Self {
        melography_lut::LutError::from(e).into()
    }
}

/// Invalid command arguments (fixed-vocabulary strings from the UI).
impl From<String> for AppError {
    fn from(message: String) -> Self {
        AppError::new("param.invalid", message)
    }
}

impl From<melography_io::IoError> for AppError {
    fn from(e: melography_io::IoError) -> Self {
        use melography_io::IoError as E;
        let code = match &e {
            E::Bomb { .. } => "io.bomb",
            E::Unsupported(_) => "io.unsupported",
            E::Decode(_) => "io.decode",
            E::Encode(_) => "io.encode",
        };
        AppError::new(code, format!("{e}"))
    }
}

impl From<melography_presets::RecipeError> for AppError {
    fn from(e: melography_presets::RecipeError) -> Self {
        match e {
            melography_presets::RecipeError::UnsupportedVersion(v) => AppError::new(
                "recipe.version",
                format!("sidecar version {v} is newer than this build"),
            ),
            melography_presets::RecipeError::Io(detail) => {
                AppError::new("recipe.io", format!("sidecar I/O: {detail}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lut_errors_map_to_stable_codes() {
        let e: AppError = melography_lut::LutError::Missing { hash: "abc".into() }.into();
        assert_eq!(e.code, "lut.missing");
        assert_eq!(e.context.as_deref(), Some("abc"));
        let e: AppError = melography_lut::LutError::EncodingNotImplemented("agx-log").into();
        assert_eq!(e.code, "lut.encoding_unimplemented");
        // Round-trips through serde for the IPC boundary.
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"lut.encoding_unimplemented\""));
    }
}
