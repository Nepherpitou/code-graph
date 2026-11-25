use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphStats {
    pub nodes: u32,
    pub functions: u32,
    pub constants: u32,
    pub edges: u32,
    pub data_edges: u32,
    pub external_edges: u32,
    pub density: f64,
    pub avg_fan_in: f64,
    pub avg_fan_out: f64,
    pub max_fan_in: u32,
    pub max_fan_out: u32,
    pub cycles: u32,
    pub nodes_in_cycles: u32,
    pub largest_cycle: u32,
    pub dead_code: u32,
    #[serde(default)]
    pub pass_through: u32,
    #[serde(default)]
    pub pass_through_ratio: f64,
    pub isolated: u32,
    pub max_height: u32,
    pub max_depth: u32,
    pub cross_file_edges: u32,
    pub cross_file_ratio: f64,
    pub result_ignored_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphTopEntry {
    pub id: String,
    pub name: String,
    pub file: String,
    pub value: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphTop {
    pub fan_in: Vec<GraphTopEntry>,
    pub fan_out: Vec<GraphTopEntry>,
    pub page_rank: Vec<GraphTopEntry>,
    pub betweenness: Vec<GraphTopEntry>,
    pub hotspot: Vec<GraphTopEntry>,
    pub complexity: Vec<GraphTopEntry>,
    pub ifc: Vec<GraphTopEntry>,
    pub dead_code: Vec<GraphTopEntry>,
}
