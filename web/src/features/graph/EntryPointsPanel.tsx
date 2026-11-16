import type { EntryPoint } from '../../types';

interface Props {
  roots: EntryPoint[];
  totalRoots: number;
  depth: number;
  truncated: boolean;
  onSelect: (id: string) => void;
}

export function EntryPointsPanel({ roots, totalRoots, depth, truncated, onSelect }: Props) {
  if (roots.length === 0) return null;
  return (
    <section className="panel entry-panel">
      <h3>Entry points</h3>
      <p className="panel-note">
        Sorted by reachable callables (transitive fan-out), {depth}-hop overview.
        Click a root to open its neighbourhood.
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
