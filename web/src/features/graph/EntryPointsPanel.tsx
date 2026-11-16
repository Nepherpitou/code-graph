import type { EntryPoint } from '../../types';

interface Props {
  roots: EntryPoint[];
  totalRoots: number;
  truncated: boolean;
  onSelect: (id: string) => void;
}

export function EntryPointsPanel({ roots, totalRoots, truncated, onSelect }: Props) {
  if (roots.length === 0) return null;
  return (
    <section className="panel entry-panel">
      <h3>Entry points</h3>
      <p className="panel-note">
        Sorted by reachable callables (transitive fan-out).
        Click a root to open its focus slice and dependencies.
      </p>
      <ol className="entry-list">
        {roots.map((root, index) => (
          <li key={root.id}>
            <button
              className="entry-row"
              onClick={() => onSelect(root.id)}
              title={root.file}
            >
              <span className="entry-rank">{index + 1}</span>
              <span className="entry-name">{root.name}</span>
              <span className="entry-reach" title="reachable callables">{root.reach}</span>
            </button>
          </li>
        ))}
      </ol>
      <p className="panel-note">
        Top {roots.length} of {totalRoots} roots · click a root to drill in
        {truncated ? ' · graph capped by node limit' : ''}
      </p>
    </section>
  );
}
