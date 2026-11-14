import { useMemo, useState } from 'react';

import type { Overview, OverviewNode } from '../../types';

interface Props {
  overview: Overview;
  totalNodes: number;
  onOpen: (directory: OverviewNode) => void;
}

export function DirectoryScreen({ overview, totalNodes, onOpen }: Props) {
  const [query, setQuery] = useState('');

  const { directories, calls } = useMemo(() => {
    const calls = new Map<string, { incoming: number; outgoing: number }>();
    for (const edge of overview.edges) {
      const source = calls.get(edge.source) ?? { incoming: 0, outgoing: 0 };
      source.outgoing += edge.total;
      calls.set(edge.source, source);
      const target = calls.get(edge.target) ?? { incoming: 0, outgoing: 0 };
      target.incoming += edge.total;
      calls.set(edge.target, target);
    }
    const needle = query.trim().toLowerCase();
    const directories = needle
      ? overview.nodes.filter((node) => node.path.toLowerCase().includes(needle))
      : overview.nodes;
    return { directories, calls };
  }, [overview, query]);

  return (
    <section className="directory-screen">
      <div className="directory-head">
        <div>
          <h2>Directories</h2>
          <p>
            {overview.nodes.length} directories · {totalNodes} nodes. Click a directory to browse
            its files. Any function can be found in the table on the left.
          </p>
        </div>
        <input
          className="server-input directory-filter"
          type="search"
          value={query}
          placeholder="filter directories…"
          onChange={(event) => setQuery(event.target.value)}
        />
      </div>

      <div className="directory-table">
        <div className="directory-row directory-header">
          <span>Directory</span>
          <span>Nodes</span>
          <span>Callables</span>
          <span>Constants</span>
          <span>Calls out / in</span>
          <span>Dead</span>
          <span>Cycles</span>
        </div>
        {directories.map((directory) => {
          const degree = calls.get(directory.id) ?? { incoming: 0, outgoing: 0 };
          return (
            <button
              key={directory.id}
              className="directory-row"
              title={`Open ${directory.path}`}
              onClick={() => onOpen(directory)}
            >
              <span className="directory-name">{directory.path === '.' ? '(root)' : directory.path}</span>
              <span>{directory.nodes}</span>
              <span>{directory.callables}</span>
              <span>{directory.constants}</span>
              <span>{degree.outgoing} / {degree.incoming}</span>
              <span className={directory.deadCode > 0 ? 'directory-warn' : ''}>{directory.deadCode}</span>
              <span className={directory.cycleNodes > 0 ? 'directory-danger' : ''}>{directory.cycleNodes}</span>
            </button>
          );
        })}
        {directories.length === 0 ? (
          <div className="directory-empty">No directories match “{query}”.</div>
        ) : null}
      </div>
    </section>
  );
}
