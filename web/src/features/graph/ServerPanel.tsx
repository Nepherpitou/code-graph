import { useState } from 'react';

import { fetchImpact, fetchPath, fetchSubgraph, searchNodes } from '../../lib/api';
import type { GraphNode, GraphSlice, PathResult } from '../../types';

interface Props {
  base: string;
  graphId: string;
  selectedId: string | null;
  onExpand: (slice: GraphSlice, focus: Set<string>) => void;
  onPath: (result: PathResult) => void;
  onError: (message: string) => void;
}

async function resolveNode(base: string, graphId: string, value: string): Promise<GraphNode | null> {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const hits = await searchNodes(base, graphId, trimmed, 1);
  return hits[0] ?? null;
}

export function ServerPanel({ base, graphId, selectedId, onExpand, onPath, onError }: Props) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<GraphNode[]>([]);
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const runSearch = async (): Promise<void> => {
    if (!query.trim()) return;
    setBusy(true);
    setMessage(null);
    try {
      setResults(await searchNodes(base, graphId, query, 20));
    } catch (caught) {
      onError((caught as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const openNode = async (node: GraphNode): Promise<void> => {
    setBusy(true);
    setMessage(null);
    try {
      const slice = await fetchSubgraph(base, graphId, [node.id], 1, 300);
      onExpand(slice, new Set([node.id]));
      setFrom(node.qname);
      setMessage(`${node.qname} loaded with ${slice.nodes.length} nodes`);
    } catch (caught) {
      onError((caught as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const findPath = async (): Promise<void> => {
    setBusy(true);
    setMessage(null);
    try {
      const [start, end] = await Promise.all([
        resolveNode(base, graphId, from),
        resolveNode(base, graphId, to),
      ]);
      if (!start || !end) {
        setMessage('Both endpoints must match a node');
        return;
      }
      const result = await fetchPath(base, graphId, start.id, end.id, 16);
      if (!result.found) {
        setMessage(`No call path ${start.qname} → ${end.qname}`);
        return;
      }
      setFrom(start.qname);
      setTo(end.qname);
      onPath(result);
      setMessage(`Path: ${result.path.length} nodes, ${result.edges.length} edges`);
    } catch (caught) {
      onError((caught as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const findImpact = async (): Promise<void> => {
    if (!selectedId) {
      setMessage('Select a node first');
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const slice = await fetchImpact(base, graphId, selectedId, 2);
      onExpand(slice, new Set(slice.nodes.map((node) => node.id)));
      setMessage(`Impact: ${slice.nodes.length} callers`);
    } catch (caught) {
      onError((caught as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel server-panel">
      <h3>Server query</h3>
      <div className="server-search">
        <input
          className="server-input"
          type="search"
          value={query}
          placeholder="name, qname, file…"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') void runSearch();
          }}
        />
        <button className="button" disabled={busy} onClick={() => void runSearch()}>Search</button>
      </div>
      {results.length > 0 ? (
        <div className="server-results">
          {results.slice(0, 8).map((node) => (
            <button key={node.id} className="server-result" onClick={() => void openNode(node)}>
              <span>{node.qname}</span>
              <small>{node.file}:{node.line}</small>
            </button>
          ))}
        </div>
      ) : null}

      <div className="server-path">
        <label className="field">
          <span>From</span>
          <input
            className="server-input"
            value={from}
            placeholder="node id or name"
            onChange={(event) => setFrom(event.target.value)}
          />
        </label>
        <label className="field">
          <span>To</span>
          <input
            className="server-input"
            value={to}
            placeholder="node id or name"
            onChange={(event) => setTo(event.target.value)}
          />
        </label>
        <div className="server-actions">
          <button className="button" disabled={busy || !from.trim() || !to.trim()} onClick={() => void findPath()}>
            Find path
          </button>
          <button className="button" disabled={busy || !selectedId} onClick={() => void findImpact()}>
            Impact
          </button>
        </div>
      </div>
      {message ? <p className="server-message">{message}</p> : null}
    </section>
  );
}
