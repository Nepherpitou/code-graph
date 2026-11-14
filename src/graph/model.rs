use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Function,
    Method,
    Constructor,
    Constant,
    Variable,
    Other,
}

impl NodeKind {
    pub fn is_callable(self) -> bool {
        matches!(
            self,
            NodeKind::Function | NodeKind::Method | NodeKind::Constructor
        )
    }

    pub fn is_data(self) -> bool {
        matches!(self, NodeKind::Constant | NodeKind::Variable)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Function => "function",
            NodeKind::Method => "method",
            NodeKind::Constructor => "constructor",
            NodeKind::Constant => "constant",
            NodeKind::Variable => "variable",
            NodeKind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetrics {
    pub fan_in: u32,
    pub fan_out: u32,
    pub call_in: u32,
    pub call_out: u32,
    pub transitive_fan_in: u32,
    pub transitive_fan_out: u32,
    pub depth: u32,
    pub height: u32,
    pub scc: i64,
    pub scc_size: u32,
    pub in_cycle: bool,
    pub page_rank: f64,
    pub betweenness: f64,
    pub instability: f64,
    pub result_ignored_ratio: f64,
    pub consumes_results: u32,
    pub provides_results: u32,
    pub external_fan_out: u32,
    pub constants_used: u32,
    pub module_refs: u32,
    pub ifc: f64,
    pub dead_code_candidate: bool,
    pub hotspot: f64,
}

impl Default for NodeMetrics {
    fn default() -> Self {
        Self {
            fan_in: 0,
            fan_out: 0,
            call_in: 0,
            call_out: 0,
            transitive_fan_in: 0,
            transitive_fan_out: 0,
            depth: 0,
            height: 0,
            scc: -1,
            scc_size: 1,
            in_cycle: false,
            page_rank: 0.0,
            betweenness: 0.0,
            instability: 0.0,
            result_ignored_ratio: 0.0,
            consumes_results: 0,
            provides_results: 0,
            external_fan_out: 0,
            constants_used: 0,
            module_refs: 0,
            ifc: 0.0,
            dead_code_candidate: false,
            hotspot: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub qname: String,
    pub kind: NodeKind,
    pub file: String,
    pub line: u32,
    pub end_line: u32,
    pub loc: u32,
    pub params: u32,
    pub cyclomatic: u32,
    pub exported: bool,
    pub external: bool,
    pub lazy: bool,
    pub synthetic: bool,
    pub is_entry_like: bool,
    pub declaration: String,
    pub metrics: NodeMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallSite {
    pub file: String,
    pub line: u32,
    pub character: u32,
    pub result_used: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultUsage {
    #[serde(rename = "none")]
    Ignored,
    Some,
    All,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(rename = "kind")]
    pub kind: EdgeKind,
    pub sites: u32,
    pub result_used: ResultUsage,
    pub external: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inferred: Option<bool>,
    pub call_sites: Vec<CallSite>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    Call,
    Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub file: String,
    pub nodes: u32,
    pub call_in: u32,
    pub call_out: u32,
    pub constants_used: u32,
    pub dead_code: u32,
}
