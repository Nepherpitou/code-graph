import type { GraphEdge, GraphNode, GraphReport } from '../../types';

interface Props {
  report: GraphReport;
  node: GraphNode | null;
  onSelect: (id: string) => void;
  onClearSelection: () => void;
}

function fmt(value: number): string {
  if (!Number.isFinite(value)) return '—';
  if (value === 0) return '0';
  if (Math.abs(value) >= 1000) return value.toFixed(0);
  return value.toFixed(2).replace(/\.?0+$/, '');
}

export function ImpactDetails({ report, node, onSelect, onClearSelection }: Props) {
  if (!node) {
    return (
      <section className="panel details" id="details">
        <h3>Impact</h3>
        <p className="muted">Select a node in the graph or the node table to load its reverse callers.</p>
        <ul className="hints">
          <li><b>Impact</b> is a mode of the graph, not a separate screen.</li>
          <li>Click a node to replace the target.</li>
          <li>The graph is scoped to the target and its incoming callers.</li>
        </ul>
      </section>
    );
  }

  const nodes = new Map(report.nodes.map((candidate) => [candidate.id, candidate]));
  const incoming = report.edges.filter((edge) => edge.target === node.id && edge.kind === 'call');
  const dataIn = report.edges.filter((edge) => edge.target === node.id && edge.kind === 'data');
  const callers = incoming
    .map((edge) => ({ edge, other: nodes.get(edge.source) }))
    .filter((entry): entry is { edge: GraphEdge; other: GraphNode } => Boolean(entry.other))
    .sort((left, right) => left.other.qname.localeCompare(right.other.qname)
      || left.edge.sites - right.edge.sites
      || left.edge.id.localeCompare(right.edge.id));

  return (
    <section className="panel details impact-details" id="details">
      <div className="details-head">
        <div>
          <div className="kind-line">
            <span className="kind kind-impact">impact</span>
            <span className="loc">{node.file}:{node.line}</span>
          </div>
          <h2>{node.qname}</h2>
        </div>
        <button className="button small" onClick={onClearSelection} title="Clear selection (Esc)">×</button>
      </div>
      <p className="impact-lede">Reverse callers loaded into the current graph slice. The target stays selected.</p>
      <div className="metrics-grid impact-metrics">
        <div className="metric"><span className="metric-label">direct callers</span><span className="metric-value">{fmt(node.metrics.fanIn)}</span></div>
        <div className="metric"><span className="metric-label">call sites in</span><span className="metric-value">{fmt(node.metrics.callIn)}</span></div>
        <div className="metric"><span className="metric-label">transitive in</span><span className="metric-value">{fmt(node.metrics.transitiveFanIn)}</span></div>
        <div className="metric"><span className="metric-label">hotspot</span><span className="metric-value">{node.metrics.hotspot.toFixed(2)}</span></div>
      </div>
      <div className="relations">
        <div className="relation-group">
          <h4>Affected callers <span className="count">{callers.length}</span></h4>
          {callers.length > 0 ? callers.slice(0, 40).map(({ edge, other }) => (
            <button key={edge.id} className="relation-row" onClick={() => onSelect(other.id)} title={`${other.file}:${other.line}`}>
              <span className={`edge-badge ${edge.resultUsed}`}>{edge.resultUsed === 'none' ? '←' : '↔'}</span>
              <span className="relation-name">{other.qname}</span>
              <span className="relation-meta">{edge.sites}×</span>
            </button>
          )) : <p className="muted">No incoming call edges in the loaded slice.</p>}
        </div>
        {dataIn.length > 0 ? (
          <div className="relation-group">
            <h4>Data references <span className="count">{dataIn.length}</span></h4>
            {[...dataIn]
              .sort((left, right) => {
                const leftName = nodes.get(left.source)?.qname ?? '';
                const rightName = nodes.get(right.source)?.qname ?? '';
                return leftName.localeCompare(rightName) || left.id.localeCompare(right.id);
              })
              .slice(0, 20)
              .map((edge) => {
              const other = nodes.get(edge.source);
              if (!other) return null;
              return (
                <button key={edge.id} className="relation-row" onClick={() => onSelect(other.id)} title={`${other.file}:${other.line}`}>
                  <span className="edge-badge data">⇢</span>
                  <span className="relation-name">{other.qname}</span>
                  <span className="relation-meta">{edge.sites}×</span>
                </button>
              );
            })}
          </div>
        ) : null}
      </div>
    </section>
  );
}
