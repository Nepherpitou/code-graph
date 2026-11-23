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
        AuditDiffView::SilentFiles => "silent-files",
    }
}

pub(super) fn output_rows(
    input: &ViewInput<'_>,
    view: AuditDiffView,
    headers: &[&str],
    widths: &[usize],
    rows: Vec<(Vec<String>, Value)>,
    extra: Value,
) -> Result<ExitCode> {
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
    output_rows(
        input,
        AuditDiffView::Uses,
        &["existing symbol", "new calls"],
        &[52, 10],
        shown
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
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
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
    output_rows(
        input,
        AuditDiffView::Dependencies,
        &["from", "to", "new calls"],
        &[24, 24, 10],
        shown
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
        json!({
            "totalRows": rows.len(),
            "truncated": rows.len() > shown.len(),
            "mutual": mutual.iter().map(|(from, to)| json!([from, to])).collect::<Vec<_>>(),
        }),
    )
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
    output_rows(
        input,
        AuditDiffView::Callers,
        &["existing caller", "now calls"],
        &[44, 44],
        shown
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
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
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
    output_rows(
        input,
        AuditDiffView::Affected,
        &["existing symbol", "changed", "added", "share", "priority", "direct"],
        &[44, 8, 6, 8, 10, 6],
        shown
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
        json!({
            "totalRows": rows.len(),
            "affectedByChanged": changed_total,
            "affectedByAddedOnly": added_only,
            "direct": direct.len(),
            "truncated": rows.len() > shown.len(),
        }),
    )
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
    output_rows(
        input,
        AuditDiffView::Areas,
        &["area", "added", "removed", "changed", "cyclomatic/callable", "loc delta"],
        &[24, 7, 8, 8, 20, 10],
        shown
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
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
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
    output_rows(
        input,
        AuditDiffView::FilteredEdges,
        &["reason", "edges"],
        &[20, 8],
        shown
            .iter()
            .map(|(reason, count)| {
                (
                    vec![(*reason).to_string(), count.to_string()],
                    json!({"reason": reason, "edges": count}),
                )
            })
            .collect(),
        json!({"newCallEdges": total, "used": real.len(), "truncated": rows.len() > shown.len()}),
    )
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
    output_rows(
        input,
        AuditDiffView::Cycles,
        &["status", "size", "members"],
        &[10, 6, 120],
        shown
            .iter()
            .map(|(members, status)| {
                (
                    vec![(*status).to_string(), members.len().to_string(), members.join(", ")],
                    json!({"status": status, "size": members.len(), "members": members}),
                )
            })
            .collect(),
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
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
    output_rows(
        input,
        AuditDiffView::DeadCode,
        &["symbol", "kind", "file", "framework"],
        &[40, 10, 40, 10],
        shown
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
        json!({
            "totalRows": rows.len(),
            "truncated": rows.len() > shown.len(),
            "constantsTruncated": constants_truncated,
            "constantsTotal": input.to.meta.constants_total,
            "constantsCrawled": input.to.meta.constants_crawled,
        }),
    )
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
    output_rows(
        input,
        AuditDiffView::SilentFiles,
        &["file", "reason"],
        &[64, 18],
        shown
            .iter()
            .map(|(file, reason)| {
                (
                    vec![(*file).to_string(), (*reason).to_string()],
                    json!({"file": file, "reason": reason}),
                )
            })
            .collect(),
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
}
