use std::path::PathBuf;

use crate::languages::LanguageId;

#[derive(Clone)]
pub struct AnalyzeOptions {
    pub root: PathBuf,
    pub lang: Option<LanguageId>,
    pub server_cmd: Option<String>,
    pub include_external: bool,
    pub include_constants: bool,
    pub ref_fallback: bool,
    pub max_nodes: usize,
    pub max_constants: usize,
    pub exclude: Vec<PathBuf>,
    pub verbose: bool,
    pub quiet: bool,
}
