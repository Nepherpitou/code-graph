import type { GraphNode, GraphReport, PathResult } from '../../types';

interface Props {
  report: GraphReport;
  path: PathResult | null;
  pathIndex: number;
  node: GraphNode | null;
  onPathIndex: (index: number) => void;
  onClearPath: () => void;
}

function fmt(value: number): string {
  if (!Number.isFinite(value)) return '—';
  if (value === 0) return '0';
  return value.toFixed(2).replace(/\.?0+$/, '');
}

export function PathDetails({ report, path, pathIndex, node, onPathIndex, onClearPath }: Props) {
  if (!path || path.path.length === 0) {
    return (
      <section className="panel details path-details" id="details">
        <h3>Path</h3>
        <p className="muted">Choose a start and an end in the path bar, then find the shortest call path.</p>
        <ul className="hints">
          <li>The route is highlighted on the graph.</li>
          <li>Click a step to move through it.</li>
          <li>Path is a mode, not a separate screen.</li>
        </ul>
      </section>
    );
  }

  const nodes = new Map(report.nodes.map((candidate) => [candidate.id, candidate]));
  const entry = nodes.get(path.path[0]);
  const exit = nodes.get(path.path[path.path.length - 1]);
  const active = node ?? nodes.get(path.path[pathIndex]) ?? null;

  return (
    <section className="panel details path-details" id="details">
      <div className="details-head">
        <div>
          <div className="kind-line">
            <span className="kind kind-path">path</span>
            <span className="loc">{path.path.length} steps · {path.edges.length} edges</span>
          </div>
          <h2>{active?.qname ?? 'Path'}</h2>
        </div>
        <button className="button small" onClick={onClearPath} title="Clear path">×</button>
      </div>
      <div className="path-endpoints">
        <div><span className="muted">entry</span><b>{entry?.qname ?? path.path[0]}</b></div>
        <div><span className="muted">exit</span><b>{exit?.qname ?? path.path[path.path.length - 1]}</b></div>
      </div>
      <div className="relations">
        <div className="relation-group">
          <h4>Route <span className="count">{path.path.length}</span></h4>
          {path.path.map((id, index) => {
            const step = nodes.get(id);
            return (
              <button
                key={`${id}:${index}`}
                className={`relation-row ${index === pathIndex ? 'path-active' : ''}`}
                onClick={() => onPathIndex(index)}
                title={step ? `${step.file}:${step.line}` : id}
              >
                <span className="edge-badge path-step-num">{index + 1}</span>
                <span className="relation-name">{step?.qname ?? id}</span>
                <span className="relation-meta">{index === pathIndex ? 'current' : step?.file.split('/').at(-1) ?? ''}</span>
              </button>
            );
          })}
        </div>
      </div>
      {active ? (
        <div className="metrics-grid path-metrics">
          <div className="metric"><span className="metric-label">fan-in</span><span className="metric-value">{fmt(active.metrics.fanIn)}</span></div>
          <div className="metric"><span className="metric-label">fan-out</span><span className="metric-value">{fmt(active.metrics.fanOut)}</span></div>
          <div className="metric"><span className="metric-label">hotspot</span><span className="metric-value">{active.metrics.hotspot.toFixed(2)}</span></div>
          <div className="metric"><span className="metric-label">LOC</span><span className="metric-value">{fmt(active.loc)}</span></div>
        </div>
      ) : null}
    </section>
  );
}
