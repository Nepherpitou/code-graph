import { useEffect, useMemo, useState } from 'react';

import type { GraphNode, PathResult } from '../../types';
import type { GraphMode } from './useGraphView';

interface Props {
  mode: GraphMode;
  onMode: (mode: GraphMode) => void;
  nodes: GraphNode[];
  selectedNode: GraphNode | null;
  path: PathResult | null;
  pathIndex: number;
  onPathIndex: (index: number) => void;
  onFindPath: (fromId: string, toId: string) => Promise<boolean>;
  apiReady: boolean;
}

const MODES: Array<[GraphMode, string]> = [
  ['focus', 'Focus'],
  ['path', 'Path'],
  ['impact', 'Impact'],
];

export function GraphModeBar({
  mode,
  onMode,
  nodes,
  selectedNode,
  path,
  pathIndex,
  onPathIndex,
  onFindPath,
  apiReady,
}: Props) {
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [busy, setBusy] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const orderedNodes = useMemo(
    () => [...nodes].sort((left, right) => {
      const entry = Number(right.isEntryLike) - Number(left.isEntryLike);
      return entry || left.qname.localeCompare(right.qname);
    }),
    [nodes],
  );
  const nodeById = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);

  useEffect(() => {
    if (!from && orderedNodes.length > 0) setFrom(orderedNodes[0].id);
    if (!to && orderedNodes.length > 0) {
      setTo(orderedNodes[Math.min(1, orderedNodes.length - 1)].id);
    }
  }, [from, to, orderedNodes]);

  useEffect(() => {
    if (selectedNode?.id) setTo(selectedNode.id);
  }, [selectedNode?.id]);

  useEffect(() => {
    if (!path) return;
    setFrom(path.path[0] ?? '');
    setTo(path.path[path.path.length - 1] ?? '');
    setPlaying(false);
  }, [path]);

  useEffect(() => {
    if (!playing || !path) return undefined;
    const timer = window.setInterval(() => {
      if (pathIndex >= path.path.length - 1) {
        setPlaying(false);
        return;
      }
      onPathIndex(pathIndex + 1);
    }, 900);
    return () => window.clearInterval(timer);
  }, [playing, path, pathIndex, onPathIndex]);

  const find = async (): Promise<void> => {
    if (!from || !to) {
      setMessage('Choose both endpoints');
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const found = await onFindPath(from, to);
      setMessage(found ? 'Path loaded' : 'No call path for those endpoints');
      if (found) setPlaying(false);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="graph-modebar">
        <span className="graph-mode-label">Mode</span>
        <div className="graph-modes">
          {MODES.map(([id, label]) => (
            <button
              key={id}
              className={`graph-mode ${mode === id ? 'active' : ''}`}
              type="button"
              aria-pressed={mode === id}
              onClick={() => onMode(id)}
            >
              <i className={`graph-mode-dot ${id}`} />
              {label}
            </button>
          ))}
        </div>
        <span className="graph-mode-hint">
          {mode === 'focus'
            ? 'select a node · path and impact stay inside this canvas'
            : mode === 'path'
              ? 'entry → exit, step by step'
              : 'reverse callers and impact for one node'}
        </span>
        <span className="graph-mode-count">
          {mode === 'path' && path ? `${pathIndex + 1} / ${path.path.length}` : null}
          {mode === 'impact' && selectedNode ? `${selectedNode.metrics.fanIn} direct callers` : null}
        </span>
      </div>

      {mode === 'path' ? (
        <div className="graph-mode-context">
          <span className="graph-context-label">Path</span>
          <select
            className="graph-select"
            value={from}
            onChange={(event) => setFrom(event.target.value)}
            aria-label="Path start"
          >
            {orderedNodes.map((node) => (
              <option key={node.id} value={node.id}>{node.qname}</option>
            ))}
          </select>
          <span className="graph-context-arrow">→</span>
          <select
            className="graph-select"
            value={to}
            onChange={(event) => setTo(event.target.value)}
            aria-label="Path end"
          >
            {orderedNodes.map((node) => (
              <option key={node.id} value={node.id}>{node.qname}</option>
            ))}
          </select>
          <button
            className="graph-mode-action"
            type="button"
            disabled={busy || !apiReady || !from || !to}
            title={apiReady ? 'Find the shortest call path' : 'Path needs the API server'}
            onClick={() => void find()}
          >
            {busy ? 'Finding…' : 'Find path'}
          </button>
          <button
            className="graph-mode-icon"
            type="button"
            disabled={!path}
            title={playing ? 'Pause path' : 'Play path'}
            onClick={() => setPlaying((current) => !current)}
          >
            {playing ? '❚❚' : '▶'}
          </button>
          {path ? (
            <div className="graph-path-strip">
              {path.path.map((id, index) => (
                <button
                  key={`${id}:${index}`}
                  className={`graph-path-step ${index === pathIndex ? 'active' : ''}`}
                  type="button"
                  title={nodeById.get(id)?.qname ?? id}
                  onClick={() => onPathIndex(index)}
                >
                  <span>{index + 1}</span>
                  {nodeById.get(id)?.name ?? id}
                </button>
              ))}
            </div>
          ) : (
            <span className="graph-context-note">
              {apiReady ? 'Choose endpoints, then find the shortest call path.' : 'Path mode needs the API server.'}
            </span>
          )}
          {message ? <span className="graph-context-status">{message}</span> : null}
        </div>
      ) : null}

      {mode === 'impact' ? (
        <div className="graph-mode-context">
          <span className="graph-context-label">Impact</span>
          {selectedNode ? (
            <>
              <b className="graph-context-name">{selectedNode.qname}</b>
              <span className="graph-context-chip">{selectedNode.metrics.fanIn} direct</span>
              <span className="graph-context-chip">{selectedNode.metrics.transitiveFanIn} transitive in</span>
              <span className="graph-context-chip danger">hotspot {selectedNode.metrics.hotspot.toFixed(2)}</span>
              <span className="graph-context-note">reverse callers are loaded from the API</span>
            </>
          ) : (
            <span className="graph-context-note">Select a node to load its reverse callers.</span>
          )}
        </div>
      ) : null}
    </>
  );
}
