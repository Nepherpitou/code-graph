use std::collections::HashMap;
use std::time::Duration;

use futures::stream::{self, StreamExt};

use crate::lsp::{LspClient, LspPosition};
use crate::progress::Reporter;
use crate::source::ast::result_used_at;

use super::Builder;
use crate::graph::model::{CallSite, EdgeKind};

impl Builder {
    pub(crate) async fn crawl_constants(
        &mut self,
        client: &LspClient,
        max_constants: usize,
        is_rust: bool,
        reporter: &Reporter,
    ) {
        let mut constants: Vec<(String, String, LspPosition)> = self
            .order
            .iter()
            .filter_map(|id| {
                self.records.get(id).and_then(|record| {
                    if record.node.kind.is_data() && !record.node.external {
                        Some((id.clone(), record.uri.clone(), record.selection.start))
                    } else {
                        None
                    }
                })
            })
            .collect();
        let total_constants = constants.len();
        constants.truncate(max_constants);
        if constants.len() < total_constants {
            self.warnings.push(format!(
                "constant reference crawl limited to {} of {total_constants} constants (use --max-constants)",
                constants.len()
            ));
        }
        reporter.emit(
            "references",
            format!("collecting references for {} constants", constants.len()),
        );
        let references = stream::iter(constants.iter().map(
            |(id, uri, position): &(String, String, LspPosition)| {
                let id = id.clone();
                let uri = uri.clone();
                let position = *position;
                async move {
                    let mut result = client
                        .references(&uri, position, false)
                        .await
                        .map_err(|error| error.to_string());
                    if is_rust && result.as_ref().map(|refs| refs.is_empty()).unwrap_or(false) {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        result = client
                            .references(&uri, position, false)
                            .await
                            .map_err(|error| error.to_string());
                    }
                    (id, result)
                }
            },
        ))
        .buffer_unordered(6)
        .collect::<Vec<_>>()
        .await;

        for (constant_id, result) in references {
            match result {
                Ok(locations) => {
                    for location in locations {
                        let Some(rel) = self.to_relative(&location.uri) else {
                            continue;
                        };
                        let declaration = self
                            .records
                            .get(&constant_id)
                            .map(|record| record.selection.start)
                            .unwrap_or_default();
                        let declaration_rel = self
                            .records
                            .get(&constant_id)
                            .map(|record| record.rel.as_str())
                            .unwrap_or("");
                        if rel == declaration_rel
                            && location.range.start.line == declaration.line
                            && location.range.start.character == declaration.character
                        {
                            continue;
                        }
                        let site = CallSite {
                            file: rel.clone(),
                            line: location.range.start.line + 1,
                            character: location.range.start.character,
                            result_used: None,
                        };
                        if let Some(caller) = self.caller_in_file(&rel, location.range.start) {
                            self.add_data_edge(&caller, &constant_id, site);
                        } else if let Some(record) = self.records.get_mut(&constant_id) {
                            record.node.metrics.module_refs += 1;
                        }
                    }
                }
                Err(error) => self
                    .warnings
                    .push(format!("references failed for {constant_id}: {error}")),
            }
        }
    }

    pub(crate) async fn fallback_references(
        &mut self,
        client: &LspClient,
        is_rust: bool,
        reporter: &Reporter,
    ) {
        let mut incoming: HashMap<String, u32> = HashMap::new();
        for edge in self.edges.values() {
            if edge.kind == EdgeKind::Call && !edge.external {
                *incoming.entry(edge.target.clone()).or_default() += 1;
            }
        }
        let candidates: Vec<(String, String, LspPosition)> = self
            .order
            .iter()
            .filter_map(|id| {
                self.records.get(id).and_then(|record| {
                    if record.node.kind.is_callable()
                        && !record.node.external
                        && incoming.get(id).copied().unwrap_or(0) == 0
                    {
                        Some((id.clone(), record.uri.clone(), record.selection.start))
                    } else {
                        None
                    }
                })
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        reporter.emit(
            "references",
            format!(
                "reference fallback for {} functions with no callers",
                candidates.len()
            ),
        );
        let references = stream::iter(candidates.iter().map(
            |(id, uri, position): &(String, String, LspPosition)| {
                let id = id.clone();
                let uri = uri.clone();
                let position = *position;
                async move {
                    let mut result = client
                        .references(&uri, position, false)
                        .await
                        .map_err(|error| error.to_string());
                    if is_rust && result.as_ref().map(|refs| refs.is_empty()).unwrap_or(false) {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        result = client
                            .references(&uri, position, false)
                            .await
                            .map_err(|error| error.to_string());
                    }
                    (id, result)
                }
            },
        ))
        .buffer_unordered(6)
        .collect::<Vec<_>>()
        .await;

        for (candidate_id, result) in references {
            match result {
                Ok(locations) => {
                    for location in locations {
                        let Some(rel) = self.to_relative(&location.uri) else {
                            continue;
                        };
                        let site = {
                            let used = self.sources.get(&rel).and_then(|source| {
                                source.tree.as_ref().and_then(|tree| {
                                    result_used_at(
                                        tree,
                                        &source.text,
                                        self.adapter,
                                        location.range.start,
                                    )
                                })
                            });
                            CallSite {
                                file: rel.clone(),
                                line: location.range.start.line + 1,
                                character: location.range.start.character,
                                result_used: used,
                            }
                        };
                        match self.caller_in_file(&rel, location.range.start) {
                            Some(caller) if caller != candidate_id => {
                                self.add_edge(&caller, &candidate_id, site, false, true);
                            }
                            Some(_) => {}
                            None => {
                                if let Some(record) = self.records.get_mut(&candidate_id) {
                                    record.node.metrics.module_refs += 1;
                                }
                            }
                        }
                    }
                }
                Err(error) => self.warnings.push(format!(
                    "references fallback failed for {candidate_id}: {error}"
                )),
            }
        }
    }
}
