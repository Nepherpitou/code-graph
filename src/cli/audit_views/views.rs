//! The audit diff views themselves.

use super::*;

pub(super) struct AffectedRow<'a> {
    node: &'a GraphNode,
    changed: u32,
    added: u32,
    all_callees: usize,
    direct: bool,
}

pub(super) struct AreaRow<'a> {
    name: &'a str,
    added: u32,
    removed: u32,
    changed: u32,
    old_mean: f64,
    new_mean: f64,
    loc_delta: i64,
}

pub(super) fn nodes_in_area<'a>(nodes: &'a [GraphNode], name: &str) -> Vec<&'a GraphNode> {
    nodes
        .iter()
        .filter(|node| area(&node.file) == name && !node.synthetic)
        .collect()
}

pub(super) fn mean_over(nodes: &[&GraphNode]) -> f64 {
    if nodes.is_empty() {
        return 0.0;
    }
    nodes.iter().map(|node| f64::from(node.cyclomatic)).sum::<f64>() / nodes.len() as f64
}

pub(super) fn envelope(view: AuditDiffView, input: &ViewInput<'_>, rows: Value, extra: Value) -> Value {
    let mut payload = json!({
        "kind": "audit.view",
        "view": view_label(view),
        "from": snapshot_json(input.from),
        "to": snapshot_json(input.to),
        "rows": rows,
        "changedFiles": input.changed_files.map(|files| files.len()),
        "limit": input.limit(),
        "truncated": false,
    });
    if let (Value::Object(payload_map), Value::Object(extra_map)) = (&mut payload, extra) {
        for (key, value) in extra_map {
            payload_map.insert(key, value);
        }
    }
    payload
}

pub(super) fn view_label(view: AuditDiffView) -> &'static str {
    match view {
        AuditDiffView::Summary => "summary",
        AuditDiffView::Symbols => "symbols",
        AuditDiffView::Uses => "uses",
        AuditDiffView::Dependencies => "dependencies",
        AuditDiffView::Callers => "callers",
        AuditDiffView::Affected => "affected",
        AuditDiffView::Areas => "areas",
        AuditDiffView::FilteredEdges => "filtered-edges",
        AuditDiffView::Cycles => "cycles",
        AuditDiffView::DeadCode => "dead-code",
        AuditDiffView::Reach => "reach",
        AuditDiffView::FanIn => "fan-in",
        AuditDiffView::Similar => "similar",
        AuditDiffView::SilentFiles => "silent-files",
    }
}

pub(super) struct TableOutput<'a, 'b> {
    pub(super) input: &'a ViewInput<'b>,
    pub(super) view: AuditDiffView,
    pub(super) headers: &'a [&'a str],
    pub(super) widths: &'a [usize],
    pub(super) rows: Vec<(Vec<String>, Value)>,
    pub(super) extra: Value,
}

pub(super) fn output_rows(output: TableOutput<'_, '_>) -> Result<ExitCode> {
    let TableOutput {
        input,
        view,
        headers,
        widths,
        rows,
        extra,
    } = output;
    let displayed = rows.len();
    let total = extra
        .get("totalRows")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(displayed);
    match input.args.format {
        AuditFormat::Json => {
            let rows_json: Vec<Value> = rows.into_iter().map(|(_, json_row)| json_row).collect();
            let mut payload = envelope(view, input, Value::Array(rows_json), extra);
            if let Some(object) = payload.as_object_mut() {
                object.insert("headers".into(), json!(headers));
            }
            print_json(&payload)?;
        }
        AuditFormat::Text => {
            println!("snapshot #{} → #{}", input.from.id, input.to.id);
            println!();
            let table: Vec<Vec<String>> = rows.into_iter().map(|(cells, _)| cells).collect();
            print_table(headers, &table, widths);
            if total > displayed {
                println!();
                println!("shown {displayed} of {total} rows · use --limit to see more");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn summary(input: &ViewInput<'_>) -> Result<ExitCode> {
    let conventions = Conventions::of(&input.to.meta.language);
    let mut old_verify = input
        .verify_calls
        .then(|| Verification::new(&input.from.meta.root));
    let mut new_verify = input
        .verify_calls
        .then(|| Verification::new(&input.to.meta.root));
    let old_real = graph_call_edges(input.old_nodes, input.old_edges, old_verify.as_mut(), conventions);
    let new_real = graph_call_edges(input.new_nodes, input.new_edges, new_verify.as_mut(), conventions);
    let cross = |edges: &[EdgeView<'_>]| {
        edges
            .iter()
            .filter(|edge| area(&edge.src.file) != area(&edge.dst.file))
            .count()
    };
    let (old_cx, old_loc, old_priority, old_dead) = totals(input.old_nodes, conventions);
    let (new_cx, new_loc, new_priority, new_dead) = totals(input.new_nodes, conventions);
    let old_cycles = sccs(input.old_nodes, input.old_edges, old_verify.as_mut(), conventions).len();
    let new_cycles = sccs(input.new_nodes, input.new_edges, new_verify.as_mut(), conventions).len();

    let raw_calls = |edges: &[GraphEdge]| edges.iter().filter(|edge| edge.kind == EdgeKind::Call).count();
    let (changed_cx_before, changed_cx_after, added_cx) = touched_complexity(input);
    let rows = vec![
        ("nodes", input.from.stats.nodes as f64, input.to.stats.nodes as f64),
        (
            "callables",
            callables(input.old_nodes).len() as f64,
            callables(input.new_nodes).len() as f64,
        ),
        (
            "call edges",
            raw_calls(input.old_edges) as f64,
            raw_calls(input.new_edges) as f64,
        ),
        ("cross-module calls", cross(&old_real) as f64, cross(&new_real) as f64),
        ("cyclomatic, total", old_cx, new_cx),
        ("cyclomatic/callable", mean_cyclomatic(input.old_nodes), mean_cyclomatic(input.new_nodes)),
        ("cyclomatic/callable (changed)", changed_cx_before, changed_cx_after),
        ("cyclomatic/callable (added)", 0.0, added_cx),
        ("loc (analyzed files)", old_loc, new_loc),
        ("refactor priority", old_priority, new_priority),
        ("real cycles", old_cycles as f64, new_cycles as f64),
        ("dead-code candidates", old_dead as f64, new_dead as f64),
    ];
    match input.args.format {
        AuditFormat::Json => {
            let rows_json: Vec<Value> = rows
                .iter()
                .map(|(label, before, after)| {
                    json!({
                        "metric": label,
                        "from": before,
                        "to": after,
                        "delta": after - before,
                    })
                })
                .collect();
            print_json(&json!({
                "kind": "audit.view",
                "view": "summary",
                "from": snapshot_json(input.from),
                "to": snapshot_json(input.to),
                "rows": rows_json,
                "rawCycles": {"from": input.from.stats.cycles, "to": input.to.stats.cycles},
            }))?;
        }
        AuditFormat::Text => {
            println!("snapshot #{} → #{}", input.from.id, input.to.id);
            println!();
            let table: Vec<Vec<String>> = rows
                .iter()
                .map(|(label, before, after)| {
                    vec![
                        (*label).to_string(),
                        format_float(*before),
                        format_float(*after),
                        format_float(after - before),
                    ]
                })
                .collect();
            print_table(&["metric", "before", "after", "delta"], &table, &[32, 12, 12, 12]);
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn uses(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let conventions = Conventions::of(&input.to.meta.language);
    let (real, _) = new_call_edges(input, graphs);
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for edge in &real {
        let dst = edge.dst;
        if dst.id.is_empty() {
            continue;
        }
        let still_exists = graphs.old_by_id.contains_key(dst.id.as_str());
        if !still_exists {
            continue;
        }
        if !is_callable(dst.kind) || dst.synthetic {
            continue;
        }
        if conventions.is_test_file(&dst.file)
            || conventions.is_framework_file(&dst.file)
            || conventions.is_shared_file(&dst.file)
        {
            continue;
        }
        *counts.entry(dst.id.as_str()).or_default() += 1;
    }
    let mut rows: Vec<(&GraphNode, u32)> = counts
        .into_iter()
        .filter_map(|(id, count)| graphs.old_by_id.get(id).map(|node| (*node, count)))
        .collect();
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.qname.cmp(&right.0.qname)));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::Uses,
        headers: &["existing symbol", "new calls"],
        widths: &[52, 10],
        rows: shown
            .iter()
            .map(|(node, count)| {
                (
                    vec![node.qname.clone(), count.to_string()],
                    json!({
                        "symbol": node.qname,
                        "file": node.file,
                        "newCalls": count,
                    }),
                )
            })
            .collect(),
        extra: json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    })
}

pub(super) fn dependencies(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let (real, _) = new_call_edges(input, graphs);
    let mut counts: HashMap<(&str, &str), u32> = HashMap::new();
    for edge in &real {
        let from = area(&edge.src.file);
        let to = area(&edge.dst.file);
        if from != to {
            *counts.entry((from, to)).or_default() += 1;
        }
    }
    let mutual: Vec<(&str, &str)> = counts
        .iter()
        .filter(|((from, to), _)| counts.contains_key(&(*to, *from)) && from < to)
        .map(|((from, to), _)| (*from, *to))
        .collect();
    let mut rows: Vec<((&str, &str), u32)> = counts.into_iter().collect();
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::Dependencies,
        headers: &["from", "to", "new calls"],
        widths: &[24, 24, 10],
        rows: shown
            .iter()
            .map(|((from, to), count)| {
                let mutual = rows
                    .iter()
                    .any(|((other_from, other_to), _)| *other_from == *to && *other_to == *from);
                (
                    vec![format!("{from}/"), format!("{to}/"), count.to_string()],
                    json!({
                        "from": from,
                        "to": to,
                        "newCalls": count,
                        "mutual": mutual,
                    }),
                )
            })
            .collect(),
        extra: json!({
            "totalRows": rows.len(),
            "truncated": rows.len() > shown.len(),
            "mutual": mutual.iter().map(|(from, to)| json!([from, to])).collect::<Vec<_>>(),
        }),
    })
}

pub(super) fn callers(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let changed = require_changed_files(input, "callers")?;
    let (real, _) = new_call_edges(input, graphs);
    let mut rows: Vec<EdgeView<'_>> = real
        .into_iter()
        .filter(|edge| changed.contains(&edge.dst.file) && !changed.contains(&edge.src.file))
        .collect();
    rows.sort_by(|left, right| {
        left.src
            .qname
            .cmp(&right.src.qname)
            .then_with(|| left.dst.qname.cmp(&right.dst.qname))
    });
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::Callers,
        headers: &["existing caller", "now calls"],
        widths: &[44, 44],
        rows: shown
            .iter()
            .map(|edge| {
                (
                    vec![edge.src.qname.clone(), edge.dst.qname.clone()],
                    json!({
                        "caller": edge.src.qname,
                        "callerFile": edge.src.file,
                        "callee": edge.dst.qname,
                        "calleeFile": edge.dst.file,
                        "sites": edge.sites,
                    }),
                )
            })
            .collect(),
        extra: json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    })
}

pub(super) fn affected(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let changed_files = require_changed_files(input, "affected")?;
    let touched = touched_ids(input, graphs);
    let mut verify = input
        .verify_calls
        .then(|| Verification::new(&input.to.meta.root));
    let conventions = Conventions::of(&input.to.meta.language);
    let (forward, reverse) = adjacency(input.new_nodes, input.new_edges, verify.as_mut(), conventions);

    let mut counts: HashMap<&str, (u32, u32)> = HashMap::new();
    for (id, status) in &touched {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();
        seen.insert(id);
        queue.push_back(id);
        while let Some(current) = queue.pop_front() {
            for caller in reverse.get(current).into_iter().flatten() {
                if seen.insert(caller) {
                    queue.push_back(caller);
                }
            }
        }
        let bucket = *status == DiffStatus::Added;
        for ancestor in seen {
            if ancestor == *id {
                continue;
            }
            let Some(node) = graphs.node(ancestor) else {
                continue;
            };
            if changed_files.contains(&node.file) {
                continue;
            }
            let entry = counts.entry(ancestor).or_default();
            if bucket {
                entry.1 += 1;
            } else {
                entry.0 += 1;
            }
        }
    }

    let direct: HashSet<&str> = counts
        .keys()
        .copied()
        .filter(|id| forward.get(id).is_some_and(|callees| {
            callees.iter().any(|callee| touched.contains_key(callee))
        }))
        .collect();

    let mut rows: Vec<AffectedRow<'_>> = counts
        .iter()
        .filter_map(|(id, (changed, added))| {
            let node = graphs.node(id)?;
            Some(AffectedRow {
                node,
                changed: *changed,
                added: *added,
                all_callees: reach_from(id, &forward).len(),
                direct: direct.contains(id),
            })
        })
        .collect();
    rows.sort_by(|left, right| {
        let left_share = f64::from(left.changed) / left.all_callees.max(1) as f64;
        let right_share = f64::from(right.changed) / right.all_callees.max(1) as f64;
        right_share
            .partial_cmp(&left_share)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.node
                    .metrics
                    .refactor_priority
                    .partial_cmp(&right.node.metrics.refactor_priority)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .reverse()
            })
            .then_with(|| left.node.qname.cmp(&right.node.qname))
    });

    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    let changed_total = counts.values().filter(|(changed, _)| *changed > 0).count();
    let added_only = counts.values().filter(|(changed, _)| *changed == 0).count();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::Affected,
        headers: &["existing symbol", "changed", "added", "share", "priority", "direct"],
        widths: &[44, 8, 6, 8, 10, 6],
        rows: shown
            .iter()
            .map(|row| {
                let share = f64::from(row.changed) / row.all_callees.max(1) as f64;
                (
                    vec![
                        row.node.qname.clone(),
                        row.changed.to_string(),
                        if row.added == 0 { String::new() } else { row.added.to_string() },
                        format!("{:.0}%", share * 100.0),
                        format_float(row.node.metrics.refactor_priority),
                        if row.direct { "yes".into() } else { String::new() },
                    ],
                    json!({
                        "symbol": row.node.qname,
                        "file": row.node.file,
                        "changedCallees": row.changed,
                        "addedCallees": row.added,
                        "allCallees": row.all_callees,
                        "share": share,
                        "priority": row.node.metrics.refactor_priority,
                        "direct": row.direct,
                    }),
                )
            })
            .collect(),
        extra: json!({
            "totalRows": rows.len(),
            "affectedByChanged": changed_total,
            "affectedByAddedOnly": added_only,
            "direct": direct.len(),
            "truncated": rows.len() > shown.len(),
        }),
    })
}

pub(super) fn areas(input: &ViewInput<'_>) -> Result<ExitCode> {
    let changed = require_changed_files(input, "areas")?;
    let mut counts: HashMap<&str, (u32, u32, u32)> = HashMap::new();
    for row in input.rows {
        if !changed.contains(&row.file) {
            continue;
        }
        let entry = counts.entry(area(&row.file)).or_default();
        match row.status {
            DiffStatus::Added => entry.0 += 1,
            DiffStatus::Removed => entry.1 += 1,
            DiffStatus::Changed | DiffStatus::Moved => entry.2 += 1,
        }
    }
    let mut rows: Vec<AreaRow<'_>> = counts
        .into_iter()
        .map(|(name, (added, removed, changed))| {
            let old_nodes = nodes_in_area(input.old_nodes, name);
            let new_nodes = nodes_in_area(input.new_nodes, name);
            let old_callables: Vec<&GraphNode> =
                old_nodes.iter().copied().filter(|node| is_callable(node.kind)).collect();
            let new_callables: Vec<&GraphNode> =
                new_nodes.iter().copied().filter(|node| is_callable(node.kind)).collect();
            let loc_delta: i64 = new_nodes.iter().map(|node| i64::from(node.loc)).sum::<i64>()
                - old_nodes.iter().map(|node| i64::from(node.loc)).sum::<i64>();
            AreaRow {
                name,
                added,
                removed,
                changed,
                old_mean: mean_over(&old_callables),
                new_mean: mean_over(&new_callables),
                loc_delta,
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        right
            .added
            .cmp(&left.added)
            .then_with(|| left.name.cmp(right.name))
    });
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::Areas,
        headers: &["area", "added", "removed", "changed", "cyclomatic/callable", "loc delta"],
        widths: &[24, 7, 8, 8, 20, 10],
        rows: shown
            .iter()
            .map(|row| {
                let cx = if row.old_mean == 0.0 || (row.old_mean - row.new_mean).abs() < 0.005 {
                    format!("{:.2}", row.new_mean)
                } else {
                    format!("{:.2} → {:.2}", row.old_mean, row.new_mean)
                };
                (
                    vec![
                        format!("{}/", row.name),
                        row.added.to_string(),
                        row.removed.to_string(),
                        row.changed.to_string(),
                        cx,
                        format!("{:+}", row.loc_delta),
                    ],
                    json!({
                        "area": row.name,
                        "added": row.added,
                        "removed": row.removed,
                        "changed": row.changed,
                        "cyclomaticBefore": row.old_mean,
                        "cyclomaticAfter": row.new_mean,
                        "locDelta": row.loc_delta,
                    }),
                )
            })
            .collect(),
        extra: json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    })
}

pub(super) fn filtered_edges(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let (real, dropped) = new_call_edges(input, graphs);
    let total: usize = input
        .new_edges
        .iter()
        .filter(|edge| {
            edge.kind == EdgeKind::Call
                && !graphs
                    .old_pairs
                    .contains(&(edge.source.as_str(), edge.target.as_str()))
        })
        .count();
    let order = [
        Bucket::Override,
        Bucket::Test,
        Bucket::Wiring,
        Bucket::Duplicate,
        Bucket::Resignature,
        Bucket::Unverified,
    ];
    let mut rows: Vec<(&str, usize)> = order
        .iter()
        .map(|bucket| (bucket.label(), dropped.get(bucket).map(Vec::len).unwrap_or(0)))
        .collect();
    rows.push(("used", real.len()));
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::FilteredEdges,
        headers: &["reason", "edges"],
        widths: &[20, 8],
        rows: shown
            .iter()
            .map(|(reason, count)| {
                (
                    vec![(*reason).to_string(), count.to_string()],
                    json!({"reason": reason, "edges": count}),
                )
            })
            .collect(),
        extra: json!({"newCallEdges": total, "used": real.len(), "truncated": rows.len() > shown.len()}),
    })
}

pub(super) fn cycles(input: &ViewInput<'_>) -> Result<ExitCode> {
    let conventions = Conventions::of(&input.to.meta.language);
    let mut old_verify = input
        .verify_calls
        .then(|| Verification::new(&input.from.meta.root));
    let mut new_verify = input
        .verify_calls
        .then(|| Verification::new(&input.to.meta.root));
    let old: HashSet<Vec<&str>> = sccs(input.old_nodes, input.old_edges, old_verify.as_mut(), conventions)
        .into_iter()
        .map(|component| {
            let mut members: Vec<&str> = component
                .iter()
                .filter_map(|id| input.old_nodes.iter().find(|n| n.id == *id))
                .map(|n| n.qname.as_str())
                .collect();
            members.sort_unstable();
            members
        })
        .collect();
    let new_cycles = sccs(input.new_nodes, input.new_edges, new_verify.as_mut(), conventions);
    let mut rows: Vec<(Vec<&str>, &'static str)> = Vec::new();
    let mut seen: HashSet<Vec<&str>> = HashSet::new();
    for component in &new_cycles {
        let mut members: Vec<&str> = component
            .iter()
            .filter_map(|id| input.new_nodes.iter().find(|n| n.id == *id))
            .map(|n| n.qname.as_str())
            .collect();
        members.sort_unstable();
        seen.insert(members.clone());
        rows.push((members.clone(), if old.contains(&members) { "existing" } else { "added" }));
    }
    for members in old {
        if !seen.contains(&members) {
            let mut members = members.clone();
            members.sort_unstable();
            rows.push((members, "removed"));
        }
    }
    rows.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then_with(|| left.0.cmp(&right.0)));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::Cycles,
        headers: &["status", "size", "members"],
        widths: &[10, 6, 120],
        rows: shown
            .iter()
            .map(|(members, status)| {
                (
                    vec![(*status).to_string(), members.len().to_string(), members.join(", ")],
                    json!({"status": status, "size": members.len(), "members": members}),
                )
            })
            .collect(),
        extra: json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    })
}

pub(super) fn dead_code(input: &ViewInput<'_>) -> Result<ExitCode> {
    let conventions = Conventions::of(&input.to.meta.language);
    // A constant outside the reference crawl has module_refs == 0 without ever
    // being looked at, so it must not be reported as dead.
    let constants_truncated = input.from.meta.constants_crawled < input.from.meta.constants_total
        || input.to.meta.constants_crawled < input.to.meta.constants_total;
    let was_dead: HashSet<(&str, &str)> = input
        .old_nodes
        .iter()
        .filter(|node| node.metrics.dead_code_candidate)
        .map(|node| (node.file.as_str(), node.qname.as_str()))
        .collect();
    let mut rows: Vec<&GraphNode> = input
        .new_nodes
        .iter()
        .filter(|node| node.metrics.dead_code_candidate)
        .filter(|node| !conventions.is_test_node(node))
        .filter(|node| !constants_truncated || !node.kind.is_data())
        .filter(|node| !was_dead.contains(&(node.file.as_str(), node.qname.as_str())))
        .collect();
    rows.sort_by(|left, right| left.qname.cmp(&right.qname));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::DeadCode,
        headers: &["symbol", "kind", "file", "framework"],
        widths: &[40, 10, 40, 10],
        rows: shown
            .iter()
            .map(|node| {
                (
                    vec![
                        node.qname.clone(),
                        node.kind.as_str().to_string(),
                        node.file.clone(),
                        if conventions.is_framework_file(&node.file) { "yes".into() } else { String::new() },
                    ],
                    json!({
                        "symbol": node.qname,
                        "kind": node.kind.as_str(),
                        "file": node.file,
                        "framework": conventions.is_framework_file(&node.file),
                    }),
                )
            })
            .collect(),
        extra: json!({
            "totalRows": rows.len(),
            "truncated": rows.len() > shown.len(),
            "constantsTruncated": constants_truncated,
            "constantsTotal": input.to.meta.constants_total,
            "constantsCrawled": input.to.meta.constants_crawled,
        }),
    })
}

/// How far the changed entry points reach now, and what they started calling.
pub(super) fn reach(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let conventions = Conventions::of(&input.to.meta.language);
    let mut verify_new = input.verify_calls.then(|| Verification::new(&input.to.meta.root));
    let mut verify_old = input
        .verify_calls
        .then(|| Verification::new(&input.from.meta.root));
    let (new_forward, new_reverse) =
        adjacency(input.new_nodes, input.new_edges, verify_new.as_mut(), conventions);
    let (old_forward, _) = adjacency(input.old_nodes, input.old_edges, verify_old.as_mut(), conventions);

    let old_by_key: HashMap<NodeKey<'_>, &str> = input
        .old_nodes
        .iter()
        .map(|node| ((node.file.as_str(), node.qname.as_str()), node.id.as_str()))
        .collect();
    let old_by_qname = qname_index(input.old_nodes);
    let old_keys: HashMap<&str, NodeKey<'_>> = input
        .old_nodes
        .iter()
        .map(|node| (node.id.as_str(), (node.file.as_str(), node.qname.as_str())))
        .collect();
    let new_keys: HashMap<&str, NodeKey<'_>> = input
        .new_nodes
        .iter()
        .map(|node| (node.id.as_str(), (node.file.as_str(), node.qname.as_str())))
        .collect();

    let mut rows: Vec<(NodeKey<'_>, DiffStatus, usize, usize, usize, usize, Vec<&str>)> = Vec::new();
    for (id, status) in touched_ids(input, graphs) {
        let Some(node) = graphs.node(id) else {
            continue;
        };
        if conventions.is_test_node(node) {
            continue;
        }
        let entry_point = new_reverse.get(id).is_none_or(Vec::is_empty)
            || conventions.is_framework_file(&node.file);
        let key = (node.file.as_str(), node.qname.as_str());
        let reach_after = reach_from(id, &new_forward).len();
        let fan_out_after = new_forward.get(id).map(Vec::len).unwrap_or(0);
        let (reach_before, fan_out_before, mut added) =
            match old_id_for(&old_by_key, &old_by_qname, key) {
            Some(old_id) => {
                let before = reach_from(old_id, &old_forward).len();
                let fan_out = old_forward.get(old_id).map(Vec::len).unwrap_or(0);
                let known: HashSet<NodeKey<'_>> = old_forward
                    .get(old_id)
                    .into_iter()
                    .flatten()
                    .filter_map(|target| old_keys.get(target).copied())
                    .collect();
                let added: Vec<&str> = new_forward
                    .get(id)
                    .into_iter()
                    .flatten()
                    .filter_map(|target| new_keys.get(target).copied())
                    .filter(|target| !known.contains(target))
                    .map(|target| target.1)
                    .collect();
                (before, fan_out, added)
            }
            None => {
                let added: Vec<&str> = new_forward
                    .get(id)
                    .into_iter()
                    .flatten()
                    .filter_map(|target| new_keys.get(target).copied())
                    .map(|target| target.1)
                    .collect();
                (0, 0, added)
            }
        };
        added.sort_unstable();
        added.dedup();
        if reach_after == reach_before && !entry_point {
            continue;
        }
        rows.push((key, status, reach_before, reach_after, fan_out_before, fan_out_after, added));
    }
    rows.sort_by(|left, right| {
        let left_delta = left.3 as i64 - left.2 as i64;
        let right_delta = right.3 as i64 - right.2 as i64;
        right_delta
            .cmp(&left_delta)
            .then_with(|| right.4.cmp(&left.4))
            .then_with(|| left.0 .1.cmp(right.0 .1))
    });
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::Reach,
        headers: &["entry point", "status", "reach", "fan-out", "new calls"],
        widths: &[40, 8, 12, 12, 40],
        rows: shown
            .iter()
            .map(|(key, status, reach_before, reach_after, before, after, added)| {
                (
                    vec![
                        key.1.to_string(),
                        status_label(*status).to_string(),
                        format!("{reach_before} → {reach_after}"),
                        format!("{before} → {after}"),
                        added.iter().take(3).copied().collect::<Vec<_>>().join(", "),
                    ],
                    json!({
                        "symbol": key.1,
                        "file": key.0,
                        "status": status_label(*status),
                        "reach": {"from": reach_before, "to": reach_after, "delta": *reach_after as i64 - *reach_before as i64},
                        "fanOut": {"from": before, "to": after},
                        "newCalls": added,
                    }),
                )
            })
            .collect(),
        extra: json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    })
}

/// Who depends on the changed symbols now: caller count, stability, and whether
/// a symbol turned into a single choke point.
pub(super) fn fan_in(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let conventions = Conventions::of(&input.to.meta.language);
    let mut verify_new = input.verify_calls.then(|| Verification::new(&input.to.meta.root));
    let mut verify_old = input
        .verify_calls
        .then(|| Verification::new(&input.from.meta.root));
    let (new_forward, new_reverse) =
        adjacency(input.new_nodes, input.new_edges, verify_new.as_mut(), conventions);
    let (old_forward, old_reverse) =
        adjacency(input.old_nodes, input.old_edges, verify_old.as_mut(), conventions);
    let old_by_key: HashMap<NodeKey<'_>, &str> = input
        .old_nodes
        .iter()
        .map(|node| ((node.file.as_str(), node.qname.as_str()), node.id.as_str()))
        .collect();
    let old_by_qname = qname_index(input.old_nodes);

    fn instability(forward: &HashMap<&str, Vec<&str>>, reverse: &HashMap<&str, Vec<&str>>, id: &str) -> f64 {
        let out = forward.get(id).map(Vec::len).unwrap_or(0) as f64;
        let incoming = reverse.get(id).map(Vec::len).unwrap_or(0) as f64;
        if out + incoming == 0.0 {
            0.0
        } else {
            out / (out + incoming)
        }
    }

    let mut rows: Vec<(NodeKey<'_>, DiffStatus, usize, usize, f64, f64, usize, bool)> = Vec::new();
    for (id, status) in touched_ids(input, graphs) {
        let Some(node) = graphs.node(id) else {
            continue;
        };
        if conventions.is_test_node(node) {
            continue;
        }
        let key = (node.file.as_str(), node.qname.as_str());
        let callers_after = new_reverse.get(id).map(Vec::len).unwrap_or(0);
        let stability_after = instability(&new_forward, &new_reverse, id);
        let modules = new_reverse
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|caller| graphs.node(caller))
            .map(|caller| area(&caller.file))
            .collect::<HashSet<_>>()
            .len();
        let (callers_before, stability_before) =
            match old_id_for(&old_by_key, &old_by_qname, key) {
            Some(old_id) => (
                old_reverse.get(old_id).map(Vec::len).unwrap_or(0),
                instability(&old_forward, &old_reverse, old_id),
            ),
            None => (0, 0.0),
        };
        let choke_point = absorbed_callers(callers_before, callers_after, modules);
        rows.push((
            key,
            status,
            callers_before,
            callers_after,
            stability_before,
            stability_after,
            modules,
            choke_point,
        ));
    }
    rows.sort_by(|left, right| {
        let left_delta = left.3 as i64 - left.2 as i64;
        let right_delta = right.3 as i64 - right.2 as i64;
        right_delta
            .cmp(&left_delta)
            .then_with(|| right.3.cmp(&left.3))
            .then_with(|| left.0 .1.cmp(right.0 .1))
    });
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::FanIn,
        headers: &["symbol", "status", "callers", "instability", "modules", "flag"],
        widths: &[40, 8, 12, 14, 8, 12],
        rows: shown
            .iter()
            .map(|(key, status, before, after, stability_before, stability_after, modules, choke)| {
                (
                    vec![
                        key.1.to_string(),
                        status_label(*status).to_string(),
                        format!("{before} → {after}"),
                        format!("{stability_before:.2} → {stability_after:.2}"),
                        modules.to_string(),
                        if *choke { "choke-point".into() } else { String::new() },
                    ],
                    json!({
                        "symbol": key.1,
                        "file": key.0,
                        "status": status_label(*status),
                        "callers": {"from": before, "to": after, "delta": *after as i64 - *before as i64},
                        "instability": {"from": stability_before, "to": stability_after},
                        "modules": modules,
                        "chokePoint": choke,
                    }),
                )
            })
            .collect(),
        extra: json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    })
}

type NodeKey<'a> = (&'a str, &'a str);

/// Old snapshot id for a target key: the same `(file, qname)` when the symbol
/// stayed put, otherwise a unique qname (moved symbol).
fn old_id_for<'a>(
    old_by_key: &HashMap<NodeKey<'a>, &'a str>,
    old_by_qname: &HashMap<&'a str, Vec<&'a str>>,
    key: NodeKey<'a>,
) -> Option<&'a str> {
    if let Some(id) = old_by_key.get(&key) {
        return Some(id);
    }
    match old_by_qname.get(key.1)?.as_slice() {
        [id] => Some(id),
        _ => None,
    }
}

fn qname_index<'a>(nodes: &'a [GraphNode]) -> HashMap<&'a str, Vec<&'a str>> {
    let mut out: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes {
        out.entry(node.qname.as_str()).or_default().push(node.id.as_str());
    }
    out
}

fn status_label(status: DiffStatus) -> &'static str {
    match status {
        DiffStatus::Changed => "changed",
        DiffStatus::Added => "added",
        DiffStatus::Removed => "removed",
        DiffStatus::Moved => "moved",
    }
}

/// A symbol comparable to another: a real callable in project code whose file
/// is not a test or a framework shell. A paired symbol has to clear the same
/// bar, otherwise every controller and DI module becomes a candidate.
fn comparable(node: &GraphNode, conventions: &Conventions) -> bool {
    is_callable(node.kind)
        && !node.synthetic
        && !node.external
        && !conventions.is_test_node(node)
        && !conventions.is_framework_file(&node.file)
        && !node.qname.ends_with(".constructor")
}

/// Outgoing call targets per symbol, keyed by node id. The LSP reports a port
/// interface and its implementation as two callees of the same call site, so a
/// name that has an abstract target keeps only the abstract ones: one semantic
/// dependency, one id.
pub(super) fn dependency_graph<'a>(
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    verify: Option<&mut Verification>,
    conventions: &Conventions,
) -> HashMap<&'a str, HashSet<&'a str>> {
    let mut grouped: HashMap<&'a str, HashMap<&'a str, Vec<&'a GraphNode>>> = HashMap::new();
    for edge in graph_call_edges(nodes, edges, verify, conventions) {
        if edge.src.id == edge.dst.id
            || !comparable(edge.src, conventions)
            || edge.dst.external
            || !is_callable(edge.dst.kind)
        {
            continue;
        }
        grouped
            .entry(edge.src.id.as_str())
            .or_default()
            .entry(edge.dst.name.as_str())
            .or_default()
            .push(edge.dst);
    }
    let mut out = HashMap::new();
    for (source, by_name) in grouped {
        let mut kept = HashSet::new();
        for targets in by_name.values() {
            let abstract_targets: Vec<&GraphNode> = targets
                .iter()
                .copied()
                .filter(|target| conventions.is_abstract(target))
                .collect();
            if abstract_targets.is_empty() {
                kept.extend(targets.iter().map(|target| target.id.as_str()));
            } else {
                kept.extend(abstract_targets.iter().map(|target| target.id.as_str()));
            }
        }
        out.insert(source, kept);
    }
    out
}

/// Both functions already called the same symbols before the change, so their
/// overlap is not new.
fn matched_in_base<'a>(
    a: &GraphNode,
    b: &GraphNode,
    old_sets: &'a HashMap<NodeKey<'a>, HashSet<NodeKey<'a>>>,
    old_by_qname: &'a HashMap<&'a str, HashSet<NodeKey<'a>>>,
) -> bool {
    let before_a = old_sets
        .get(&(a.file.as_str(), a.qname.as_str()))
        .or_else(|| old_by_qname.get(a.qname.as_str()));
    let before_b = old_sets
        .get(&(b.file.as_str(), b.qname.as_str()))
        .or_else(|| old_by_qname.get(b.qname.as_str()));
    matches!((before_a, before_b), (Some(left), Some(right)) if left == right)
}

/// A changed symbol whose direct calls largely overlap another symbol's.
pub(super) fn similar(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let conventions = Conventions::of(&input.to.meta.language);
    let mut verify_new = input.verify_calls.then(|| Verification::new(&input.to.meta.root));
    let mut verify_old = input
        .verify_calls
        .then(|| Verification::new(&input.from.meta.root));
    let new_sets = dependency_graph(input.new_nodes, input.new_edges, verify_new.as_mut(), conventions);
    let old_sets = dependency_graph(input.old_nodes, input.old_edges, verify_old.as_mut(), conventions);

    let new_by_id: HashMap<&str, &GraphNode> = input
        .new_nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let old_by_id: HashMap<&str, &GraphNode> = input
        .old_nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let old_key_sets: HashMap<NodeKey<'_>, HashSet<NodeKey<'_>>> = old_sets
        .iter()
        .filter_map(|(id, targets)| {
            let node = old_by_id.get(id)?;
            let mut keys = HashSet::new();
            for target in targets {
                let target = old_by_id.get(target)?;
                keys.insert((target.file.as_str(), target.qname.as_str()));
            }
            Some(((node.file.as_str(), node.qname.as_str()), keys))
        })
        .collect();
    let mut old_by_qname: HashMap<&str, HashSet<NodeKey<'_>>> = HashMap::new();
    for (key, targets) in &old_key_sets {
        old_by_qname.entry(key.1).or_insert_with(|| targets.clone());
    }

    let touched: HashMap<&str, DiffStatus> = touched_ids(input, graphs)
        .into_iter()
        .filter(|(id, _)| {
            graphs
                .node(id)
                .is_some_and(|node| comparable(node, conventions))
        })
        .collect();

    let rows = find_similar(&new_sets, &new_by_id, &old_key_sets, &old_by_qname, &touched);
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::Similar,
        headers: &["symbol", "similar to", "shared", "share", "calls in common", "same file"],
        widths: &[38, 38, 7, 7, 46, 8],
        rows: shown
            .iter()
            .map(|pair| {
                (
                    vec![
                        pair.a.1.to_string(),
                        pair.b.1.to_string(),
                        pair.shared.to_string(),
                        format!("{:.0}%", pair.containment * 100.0),
                        pair.shared_calls.join(", "),
                        if pair.a.0 == pair.b.0 { "yes".into() } else { String::new() },
                    ],
                    json!({
                        "a": pair.a.1,
                        "aFile": pair.a.0,
                        "aStatus": pair.left.map(status_label),
                        "b": pair.b.1,
                        "bFile": pair.b.0,
                        "bStatus": pair.right.map(status_label),
                        "shared": pair.shared,
                        "containment": pair.containment,
                        "sharedCalls": pair.shared_calls,
                        "sameFile": pair.a.0 == pair.b.0,
                    }),
                )
            })
            .collect(),
        extra: json!({
            "totalRows": rows.len(),
            "truncated": rows.len() > shown.len(),
            "thresholds": {
                "minFanOut": SIMILAR_MIN_FANOUT,
                "minShared": SIMILAR_MIN_SHARED,
                "minContainment": SIMILAR_MIN_CONTAINMENT,
            },
        }),
    })
}

/// A symbol that picked up callers from several modules in one change is a
/// choke point: every one of them now goes through it.
pub(super) fn absorbed_callers(before: usize, after: usize, modules: usize) -> bool {
    after >= 3 && after > before + 1 && modules >= 2
}

/// Calls below this count can match by accident, and a symbol with one or two
/// callees is a wrapper, not a pipeline.
pub(crate) const SIMILAR_MIN_FANOUT: usize = 3;
/// The smallest overlap worth a row: three calls into the same symbols.
pub(crate) const SIMILAR_MIN_SHARED: usize = 3;
/// Share of the smaller callee set that has to be common: half of the skeleton.
pub(crate) const SIMILAR_MIN_CONTAINMENT: f64 = 0.5;

pub(crate) struct SimilarRow<'a> {
    pub(crate) a: NodeKey<'a>,
    pub(crate) b: NodeKey<'a>,
    pub(crate) shared: usize,
    pub(crate) containment: f64,
    pub(crate) shared_calls: Vec<&'a str>,
}

/// Similar callables inside one snapshot: every comparable callable is a
/// candidate, no base snapshot and no changed-symbol filter.
pub(crate) fn all_similar_pairs<'a>(
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    conventions: &Conventions,
) -> Vec<SimilarRow<'a>> {
    let sets = dependency_graph(nodes, edges, None, conventions);
    let by_id: HashMap<&'a str, &'a GraphNode> = nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let touched: HashMap<&'a str, DiffStatus> = sets
        .keys()
        .map(|id| (*id, DiffStatus::Changed))
        .collect();
    let old_sets: HashMap<NodeKey<'a>, HashSet<NodeKey<'a>>> = HashMap::new();
    let old_by_qname: HashMap<&'a str, HashSet<NodeKey<'a>>> = HashMap::new();
    let mut rows: Vec<SimilarRow<'a>> =
        find_similar(&sets, &by_id, &old_sets, &old_by_qname, &touched)
            .into_iter()
            .map(|pair| SimilarRow {
                a: pair.a,
                b: pair.b,
                shared: pair.shared,
                containment: pair.containment,
                shared_calls: pair.shared_calls,
            })
            .collect();
    rows.sort_by(|left, right| {
        right
            .shared
            .cmp(&left.shared)
            .then_with(|| right.containment.total_cmp(&left.containment))
            .then_with(|| left.a.1.cmp(right.a.1))
            .then_with(|| left.b.1.cmp(right.b.1))
    });
    rows
}

pub(super) struct SimilarPair<'a> {
    pub(super) a: NodeKey<'a>,
    pub(super) b: NodeKey<'a>,
    pub(super) left: Option<DiffStatus>,
    pub(super) right: Option<DiffStatus>,
    pub(super) shared: usize,
    pub(super) containment: f64,
    pub(super) shared_calls: Vec<&'a str>,
}

fn inverted_callee_index<'a>(
    new_sets: &HashMap<&'a str, HashSet<&'a str>>,
) -> HashMap<&'a str, Vec<&'a str>> {
    let mut by_callee: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
    for (&source, targets) in new_sets {
        if targets.len() < SIMILAR_MIN_FANOUT {
            continue;
        }
        for callee in targets {
            by_callee.entry(*callee).or_default().push(source);
        }
    }
    by_callee
}

fn shared_counts<'a>(
    set_a: &HashSet<&'a str>,
    by_callee: &HashMap<&'a str, Vec<&'a str>>,
    source: &'a str,
) -> HashMap<&'a str, usize> {
    let mut counts: HashMap<&'a str, usize> = HashMap::new();
    for callee in set_a {
        let Some(sources) = by_callee.get(callee) else {
            continue;
        };
        for &other in sources {
            if other != source {
                *counts.entry(other).or_insert(0) += 1;
            }
        }
    }
    counts
}

fn better_partner(current: Option<&SimilarPair<'_>>, candidate: &SimilarPair<'_>) -> bool {
    current.is_none_or(|current| {
        candidate.containment > current.containment
            || (candidate.containment == current.containment
                && (candidate.shared > current.shared
                    || (candidate.shared == current.shared && candidate.b.1 < current.b.1)))
    })
}

/// For every touched symbol its closest partner in the snapshot. Both sides
/// need at least [`SIMILAR_MIN_FANOUT`] callees and at least
/// [`SIMILAR_MIN_SHARED`] of them in common; the common part must cover half of
/// the smaller set. A direct call between the two disqualifies the pair — a
/// helper and its caller share calls by construction — as does a pair that
/// already matched in the base snapshot.
pub(super) fn find_similar<'a>(
    new_sets: &HashMap<&'a str, HashSet<&'a str>>,
    new_by_id: &HashMap<&'a str, &'a GraphNode>,
    old_sets: &HashMap<NodeKey<'a>, HashSet<NodeKey<'a>>>,
    old_by_qname: &HashMap<&'a str, HashSet<NodeKey<'a>>>,
    touched: &HashMap<&'a str, DiffStatus>,
) -> Vec<SimilarPair<'a>> {
    // Inverted callee index: for each touched source, count shared callees
    // instead of intersecting every candidate pair.
    let by_callee = inverted_callee_index(new_sets);
    let mut rows: Vec<SimilarPair<'a>> = Vec::new();
    for (&source, &status) in touched {
        let Some(set_a) = new_sets.get(source) else {
            continue;
        };
        let Some(node_a) = new_by_id.get(source).copied() else {
            continue;
        };
        if set_a.len() < SIMILAR_MIN_FANOUT {
            continue;
        }
        let counts = shared_counts(set_a, &by_callee, source);
        let mut best: Option<SimilarPair<'a>> = None;
        for (other, shared) in counts {
            if shared < SIMILAR_MIN_SHARED {
                continue;
            }
            if set_a.contains(other) || new_sets[other].contains(source) {
                continue;
            }
            let set_b = &new_sets[other];
            let containment = shared as f64 / set_a.len().min(set_b.len()) as f64;
            if containment < SIMILAR_MIN_CONTAINMENT {
                continue;
            }
            let Some(node_b) = new_by_id.get(other).copied() else {
                continue;
            };
            if matched_in_base(node_a, node_b, old_sets, old_by_qname) {
                continue;
            }
            let mut shared_calls: Vec<&str> = set_a
                .intersection(set_b)
                .map(|id| new_by_id.get(id).map_or(*id, |node| node.qname.as_str()))
                .collect();
            shared_calls.sort_unstable();
            let candidate = SimilarPair {
                a: (node_a.file.as_str(), node_a.qname.as_str()),
                b: (node_b.file.as_str(), node_b.qname.as_str()),
                left: Some(status),
                right: touched.get(other).copied(),
                shared,
                containment,
                shared_calls,
            };
            if better_partner(best.as_ref(), &candidate) {
                best = Some(candidate);
            }
        }
        if let Some(pair) = best {
            rows.push(pair);
        }
    }
    let mut seen = HashSet::new();
    rows.retain(|pair| {
        let key = if pair.a <= pair.b {
            (pair.a, pair.b)
        } else {
            (pair.b, pair.a)
        };
        seen.insert(key)
    });
    for pair in &mut rows {
        let swapped = (pair.left.is_none() && pair.right.is_some())
            || (pair.left.is_some() == pair.right.is_some() && pair.b < pair.a);
        if swapped {
            std::mem::swap(&mut pair.a, &mut pair.b);
            std::mem::swap(&mut pair.left, &mut pair.right);
        }
    }
    rows.sort_by(|left, right| {
        let same_file = |pair: &SimilarPair<'a>| pair.a.0 == pair.b.0;
        same_file(right)
            .cmp(&same_file(left))
            .then_with(|| {
                right
                    .containment
                    .partial_cmp(&left.containment)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.shared.cmp(&left.shared))
            .then_with(|| left.a.1.cmp(right.a.1))
    });
    rows
}

pub(super) fn silent_files(input: &ViewInput<'_>) -> Result<ExitCode> {
    let conventions = Conventions::of(&input.to.meta.language);
    let changed = require_changed_files(input, "silent-files")?;
    let touched: HashSet<&str> = input.rows.iter().map(|row| row.file.as_str()).collect();
    let mut rows: Vec<(&str, &'static str)> = changed
        .iter()
        .filter(|file| !touched.contains(file.as_str()))
        .map(|file| {
            (file.as_str(), conventions.silent_reason(file))
        })
        .collect();
    rows.sort_by(|left, right| left.0.cmp(right.0));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(TableOutput {
        input,
        view: AuditDiffView::SilentFiles,
        headers: &["file", "reason"],
        widths: &[64, 18],
        rows: shown
            .iter()
            .map(|(file, reason)| {
                (
                    vec![(*file).to_string(), (*reason).to_string()],
                    json!({"file": file, "reason": reason}),
                )
            })
            .collect(),
        extra: json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    })
}
