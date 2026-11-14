use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use futures::future::join_all;

use crate::lsp::{CallHierarchyItem, LspClient, LspPosition, OutgoingCall};
use crate::progress::Reporter;

use super::Builder;

struct FetchInput {
    id: String,
    rel: String,
    uri: String,
    selection: LspPosition,
    range_start: LspPosition,
}

async fn fetch_calls(
    client: &LspClient,
    input: &FetchInput,
) -> Result<Vec<(CallHierarchyItem, Vec<OutgoingCall>)>, String> {
    let mut items = Vec::new();
    for attempt in 0..3 {
        items = client
            .prepare_call_hierarchy(&input.uri, input.selection)
            .await
            .map_err(|error| format!("prepareCallHierarchy failed for {}: {error}", input.rel))?;
        if items.is_empty() {
            items = client
                .prepare_call_hierarchy(&input.uri, input.range_start)
                .await
                .map_err(|error| {
                    format!("prepareCallHierarchy failed for {}: {error}", input.rel)
                })?;
        }
        if !items.is_empty() {
            break;
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    let mut result = Vec::new();
    for item in items {
        let calls = client
            .outgoing_calls(&item)
            .await
            .map_err(|error| format!("outgoingCalls failed for {}: {error}", item.name))?;
        result.push((item, calls));
    }
    Ok(result)
}

impl Builder {
    pub(crate) async fn walk_calls(&mut self, client: &LspClient, reporter: &Reporter) {
        let callable_ids: Vec<String> = self
            .order
            .iter()
            .filter(|id| {
                self.records
                    .get(*id)
                    .map(|record| record.node.kind.is_callable() && !record.node.synthetic)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let mut queue: VecDeque<String> = callable_ids.into_iter().collect();
        let mut visited: HashSet<String> = HashSet::new();
        let mut processed = 0usize;

        while !queue.is_empty() {
            let mut batch = Vec::new();
            while batch.len() < 8 {
                let Some(id) = queue.pop_front() else { break };
                if visited.insert(id.clone()) {
                    batch.push(id);
                }
            }
            if batch.is_empty() {
                break;
            }
            let mut inputs = Vec::new();
            for id in &batch {
                if let Some(record) = self.records.get(id) {
                    inputs.push(FetchInput {
                        id: id.clone(),
                        rel: record.rel.clone(),
                        uri: record.uri.clone(),
                        selection: record.selection.start,
                        range_start: record.range.start,
                    });
                }
            }
            let results = join_all(inputs.iter().map(|input| fetch_calls(client, input))).await;
            for (input, result) in inputs.iter().zip(results) {
                let calls = match result {
                    Ok(calls) => calls,
                    Err(error) => {
                        self.warnings.push(error);
                        continue;
                    }
                };
                for (_item, outgoing) in calls {
                    for call in outgoing {
                        let resolved = self.resolve_target(&call.to);
                        if resolved.external {
                            if self.include_external {
                                let rel = self.to_relative(&call.to.uri);
                                let external_id =
                                    self.ensure_external_node(&call.to, rel.as_deref());
                                if let Some(caller) = self.records.get(&input.id) {
                                    let rel = caller.rel.clone();
                                    for range in &call.from_ranges {
                                        let site = self.call_site(&rel, *range);
                                        self.add_edge(&input.id, &external_id, site, true, false);
                                    }
                                }
                                let set =
                                    self.external_callees.entry(input.id.clone()).or_default();
                                set.insert(external_id);
                            } else {
                                let set =
                                    self.external_callees.entry(input.id.clone()).or_default();
                                set.insert(format!(
                                    "{}#{}@{}:{}",
                                    call.to.uri,
                                    call.to.name,
                                    call.to.selection_range.start.line,
                                    call.to.selection_range.start.character
                                ));
                            }
                            continue;
                        }
                        let Some(target) = resolved.id else { continue };
                        for range in &call.from_ranges {
                            let site = self.call_site(&input.rel, *range);
                            self.add_edge(&input.id, &target, site, false, false);
                        }
                        if resolved.created_callable && !visited.contains(&target) {
                            queue.push_back(target);
                        }
                    }
                }
            }
            processed += batch.len();
            if processed % 64 < 8 {
                reporter.emit(
                    "calls",
                    format!(
                        "call hierarchy: {processed} functions processed, {} edges",
                        self.edge_order.len()
                    ),
                );
            }
            if self.records.len() >= self.max_nodes {
                self.truncated = true;
                break;
            }
        }
    }
}
