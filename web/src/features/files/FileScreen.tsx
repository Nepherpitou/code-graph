import { useMemo, useState } from 'react';

import type { Overview, OverviewNode } from '../../types';

interface Props {
  overview: Overview;
  directory: string;
  onOpen: (file: OverviewNode) => void;
}

function inDirectory(file: string, directory: string): boolean {
  if (directory === '.') return !file.includes('/');
  return file.startsWith(`${directory}/`);
}

export function FileScreen({ overview, directory, onOpen }: Props) {
  const [query, setQuery] = useState('');

  const { files, calls } = useMemo(() => {
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
    const files = overview.nodes.filter((node) => {
      if (!inDirectory(node.path, directory)) return false;
      return !needle || node.path.toLowerCase().includes(needle);
    });
    return { files, calls };
  }, [overview, directory, query]);

  return (
    <section className="directory-screen">
      <div className="directory-head">
        <div>
          <h2>Files in {directory === '.' ? '(root)' : directory}</h2>
          <p>
            {files.length} files. Click a file to open its function graph. Callbacks and
            cross-file edges are resolved on the graph screen.
          </p>
        </div>
        <input
          className="server-input directory-filter"
          type="search"
          value={query}
          placeholder="filter files…"
          onChange={(event) => setQuery(event.target.value)}
        />
      </div>

      <div className="directory-table">
        <div className="directory-row directory-header">
          <span>File</span>
          <span>Nodes</span>
          <span>Callables</span>
          <span>Constants</span>
          <span>Calls out / in</span>
          <span>Dead</span>
          <span>Cycles</span>
        </div>
        {files.map((file) => {
          const degree = calls.get(file.id) ?? { incoming: 0, outgoing: 0 };
          const relative = directory === '.' ? file.path : file.path.slice(directory.length + 1);
          return (
            <button
              key={file.id}
              className="directory-row"
              title={`Open ${file.path}`}
              onClick={() => onOpen(file)}
            >
              <span className="directory-name">{relative}</span>
              <span>{file.nodes}</span>
              <span>{file.callables}</span>
              <span>{file.constants}</span>
              <span>{degree.outgoing} / {degree.incoming}</span>
              <span className={file.deadCode > 0 ? 'directory-warn' : ''}>{file.deadCode}</span>
              <span className={file.cycleNodes > 0 ? 'directory-danger' : ''}>{file.cycleNodes}</span>
            </button>
          );
        })}
        {files.length === 0 ? <div className="directory-empty">No files match “{query}”.</div> : null}
      </div>
    </section>
  );
}
