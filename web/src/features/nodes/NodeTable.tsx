import { useMemo, useRef, useState } from 'react';

import type { GraphNode } from '../../types';

const ROW_HEIGHT = 32;
const VIEWPORT_HEIGHT = 320;

type SortKey = 'fanIn' | 'fanOut' | 'hotspot' | 'name';

interface Props {
  nodes: GraphNode[];
  total: number;
  onOpen: (node: GraphNode) => void;
}

function metric(node: GraphNode, key: SortKey): number | string {
  if (key === 'name') return node.qname;
  return node.metrics[key];
}

export function NodeTable({ nodes, total, onOpen }: Props) {
  const [query, setQuery] = useState('');
  const [sortKey, setSortKey] = useState<SortKey>('fanIn');
  const [scrollTop, setScrollTop] = useState(0);
  const scrollRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const rows = needle
      ? nodes.filter((node) =>
          `${node.qname} ${node.file} ${node.kind}`.toLowerCase().includes(needle),
        )
      : nodes;
    return [...rows].sort((left, right) => {
      const a = metric(left, sortKey);
      const b = metric(right, sortKey);
      const byId = left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
      if (typeof a === 'string' || typeof b === 'string') {
        return String(a).localeCompare(String(b)) || byId;
      }
      return b - a || left.qname.localeCompare(right.qname) || byId;
    });
  }, [nodes, query, sortKey]);

  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 2);
  const end = Math.min(filtered.length, start + Math.ceil(VIEWPORT_HEIGHT / ROW_HEIGHT) + 5);
  const visible = filtered.slice(start, end);

  return (
    <section className="panel node-table">
      <h3>All nodes</h3>
      <div className="node-table-controls">
        <input
          className="server-input"
          type="search"
          value={query}
          placeholder="filter all nodes…"
          onChange={(event) => {
            setQuery(event.target.value);
            setScrollTop(0);
            scrollRef.current?.scrollTo({ top: 0 });
          }}
        />
        <select
          className="node-table-sort"
          value={sortKey}
          onChange={(event) => setSortKey(event.target.value as SortKey)}
          aria-label="Sort nodes"
        >
          <option value="fanIn">fan-in</option>
          <option value="fanOut">fan-out</option>
          <option value="hotspot">hotspot</option>
          <option value="name">name</option>
        </select>
      </div>
      <div className="node-table-count">
        {filtered.length} / {total} nodes
      </div>
      <div
        className="node-table-scroll"
        ref={scrollRef}
        style={{ height: VIEWPORT_HEIGHT }}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      >
        <div style={{ height: filtered.length * ROW_HEIGHT, position: 'relative' }}>
          {visible.map((node, index) => (
            <button
              key={node.id}
              className="node-table-row"
              style={{ top: (start + index) * ROW_HEIGHT }}
              title={`${node.qname}\n${node.file}:${node.line}`}
              onClick={() => onOpen(node)}
            >
              <span className="node-table-name">{node.qname}</span>
              <span className="node-table-kind">{node.kind}</span>
              <span className="node-table-degrees">
                in {node.metrics.fanIn} · out {node.metrics.fanOut}
              </span>
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}
