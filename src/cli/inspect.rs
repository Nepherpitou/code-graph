use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::report::GraphReport;

fn pad(value: &str, width: usize) -> String {
    if value.chars().count() >= width {
        value.to_string()
    } else {
        format!("{value}{}", " ".repeat(width - value.chars().count()))
    }
}

pub(super) fn inspect(file: &Path) -> Result<()> {
    let bytes = fs::read(file)?;
    let report: GraphReport = serde_json::from_slice(&bytes)?;
    let stats = &report.stats;
    let meta = &report.meta;
    println!("Graph: {}", meta.root);
    println!(
        "  language {} via {}, {} files, {}ms",
        meta.language, meta.server, meta.files, meta.duration_ms
    );
    println!(
        "  {} nodes ({} callables, {} constants), {} call edges, {} data edges",
        stats.nodes, stats.functions, stats.constants, stats.edges, stats.data_edges
    );
    println!(
        "  density {:.4}, avg fan-in {:.2}, avg fan-out {:.2}",
        stats.density, stats.avg_fan_in, stats.avg_fan_out
    );
    println!(
        "  cycles {} ({} nodes), dead-code candidates {}, isolated {}",
        stats.cycles, stats.nodes_in_cycles, stats.dead_code, stats.isolated
    );
    println!(
        "  ignored results {:.1}%, cross-file calls {:.1}%",
        stats.result_ignored_ratio * 100.0,
        stats.cross_file_ratio * 100.0
    );
    if meta.truncated {
        println!("  WARNING: graph is truncated");
    }

    let tables: Vec<(&str, &Vec<crate::metrics::GraphTopEntry>)> = vec![
        ("fan-in (blast radius)", &report.top.fan_in),
        ("fan-out (coupling)", &report.top.fan_out),
        ("page rank / CGBR", &report.top.page_rank),
        ("betweenness", &report.top.betweenness),
        ("hotspot", &report.top.hotspot),
        ("cyclomatic complexity", &report.top.complexity),
        ("information flow (IFC)", &report.top.ifc),
    ];
    for (title, rows) in tables {
        if rows.is_empty() {
            continue;
        }
        println!("\nTop by {title}:");
        for row in rows.iter().take(8) {
            println!("  {:>8.3} {} {}", row.value, pad(&row.name, 44), row.file);
        }
    }
    if !report.top.dead_code.is_empty() {
        println!("\nDead-code candidates (never called, not exported, not tests):");
        for row in report.top.dead_code.iter().take(8) {
            println!(
                "  {:>6} loc  {} {}",
                row.value,
                pad(&row.name, 44),
                row.file
            );
        }
    }
    if !report.warnings.is_empty() {
        println!("\nWarnings:");
        for warning in &report.warnings {
            println!("  - {warning}");
        }
    }
    Ok(())
}
