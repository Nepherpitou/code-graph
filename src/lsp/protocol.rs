use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct LspError {
    pub code: Option<i64>,
    pub message: String,
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(code) => write!(f, "{} ({code})", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for LspError {}

pub(super) const RETRYABLE: [i64; 3] = [-32801, -32802, -32002]; // ContentModified, ServerCancelled, ServerNotInitialized

pub(super) fn retryable(error: &LspError) -> bool {
    error
        .code
        .map(|code| RETRYABLE.contains(&code))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyItem {
    pub name: String,
    pub kind: u32,
    #[serde(default)]
    pub detail: Option<String>,
    pub uri: String,
    pub range: LspRange,
    pub selection_range: LspRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingCall {
    pub to: CallHierarchyItem,
    #[serde(default)]
    pub from_ranges: Vec<LspRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub uri: String,
    pub range: LspRange,
}
