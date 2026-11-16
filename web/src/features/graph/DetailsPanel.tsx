import { useEffect } from 'react';
import type { GraphEdge, GraphNode, GraphReport } from '../../types';

const METRIC_HELP: Record<string, string> = {
  fanIn: 'Distinct project functions that call/reference this node (blast radius).',
  fanOut: 'Distinct project functions/constants this node calls or references.',
  callIn: 'Incoming call/reference sites, counting multiplicity.',
  callOut: 'Outgoing call/reference sites, counting multiplicity.',
  transitiveFanIn: 'How many functions can reach this one through call chains.',
  transitiveFanOut: 'How many functions are reachable from this one.',
  depth: 'Longest call chain from an entry-side source (SCC-condensed graph).',
  height: 'Longest call chain from here down to a leaf.',
  pageRank: 'PageRank / call-graph based ranking over calls + constant usage.',
  betweenness: 'How often this node lies on shortest call paths (Brandes).',
  cyclomatic: 'Decision points + 1, computed from the tree-sitter AST.',
  instability: 'fanOut / (fanIn + fanOut): 0 = stable provider, 1 = unstable consumer.',
  resultIgnoredRatio: 'Share of outgoing call sites whose return value is discarded.',
  ifc: 'Information flow complexity: LOC + (fanIn × fanOut)².',
  hotspot: 'Heuristic blend of blast radius, coupling, size, and complexity.',
  loc: 'Lines spanned by the declaration.',
  params: 'Explicit parameters (receiver excluded).',
  constantsUsed: 'Distinct constants/variables referenced by this function.',
  externalFanOut: 'Distinct library/external callees (not shown unless --include-external).',
  moduleRefs: 'References from module scope (outside any function).',
};

interface Props {
  report: GraphReport;
  node: GraphNode | null;
  onSelect: (id: string) => void;
  onFocus: (id: string) => void;
  onClearFocus: () => void;
  onClearSelection: () => void;
  focused: boolean;
}

function pct(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}

function fmt(value: number): string {
  if (!Number.isFinite(value)) return '—';
  if (value === 0) return '0';
  if (Math.abs(value) >= 1000) return value.toFixed(0);
  return value.toFixed(2).replace(/\.?0+$/, '');
}

function RelationList({
  title, edges, side, report, onSelect,
}: { title: string; edges: GraphEdge[]; side: 'source' | 'target'; report: GraphReport; onSelect: (id: string) => void }) {
  if (edges.length === 0) return null;
  const nodes = new Map(report.nodes.map((node) => [node.id, node]));
  // Relation order is part of the view, not of the API map iteration order.
  const ordered = [...edges].sort((left, right) => {
    const leftName = nodes.get(side === 'source' ? left.source : left.target)?.qname ?? '';
    const rightName = nodes.get(side === 'source' ? right.source : right.target)?.qname ?? '';
    return leftName.localeCompare(rightName) || left.sites - right.sites || left.id.localeCompare(right.id);
  });
  return (
    <div className="relation-group">
      <h4>{title} <span className="count">{edges.length}</span></h4>
      {ordered.slice(0, 40).map((edge) => {
        const other = nodes.get(side === 'source' ? edge.source : edge.target);
        if (!other) return null;
        const badge = edge.kind === 'data' ? '⇢' : edge.inferred ? '⋯' : edge.resultUsed === 'none' ? '→' : edge.resultUsed === 'unknown' ? '?' : '↔';
        const badgeClass = edge.kind === 'data' ? 'data' : edge.inferred ? 'inferred' : edge.resultUsed;
        return (
          <button key={edge.id} className="relation-row" onClick={() => onSelect(other.id)} title={`${other.file}:${other.line}`}>
            <span className={`edge-badge ${badgeClass}`}>{badge}</span>
            <span className="relation-name">{other.qname}</span>
            <span className="relation-meta">{edge.sites}×{edge.inferred ? ' refs' : ''}</span>
          </button>
        );
      })}
    </div>
  );
}

export function DetailsPanel({ report, node, onSelect, onFocus, onClearFocus, onClearSelection, focused }: Props) {
  useEffect(() => {
    document.querySelector('.sidebar.right')?.scrollTo({ top: 0 });
  }, [node?.id]);

  if (!node) {
    return (
      <section className="panel details" id="details">
        <h3>Node details</h3>
        <p className="muted">Select a node to inspect metrics, callers, and callees.</p>
        <ul className="hints">
          <li><b>Click</b> a card to focus its direct neighbours and shortest entry→exit route.</li>
          <li><b>Red path</b> is the focus-navigation history; <b>amber</b> is the shortest route.</li>
          <li><b>Double-click</b> focuses the slice and starts a fresh history.</li>
          <li><b>Top entries</b> returns to the entry-point overview.</li>
          <li>Arrowheads point at callees; a second arrowhead means the result is used.</li>
          <li>Dashed edges are reference fallback or constant usage.</li>
          <li>Press <kbd>/</kbd> to search, <kbd>Esc</kbd> to clear.</li>
        </ul>
      </section>
    );
  }

  const metrics = node.metrics;
  const incoming = report.edges.filter((edge) => edge.target === node.id);
  const outgoing = report.edges.filter((edge) => edge.source === node.id);
  const rows: Array<[string, string, string]> = [
    ['fan-in', fmt(metrics.fanIn), METRIC_HELP.fanIn],
    ['fan-out', fmt(metrics.fanOut), METRIC_HELP.fanOut],
    ['call sites in', fmt(metrics.callIn), METRIC_HELP.callIn],
    ['call sites out', fmt(metrics.callOut), METRIC_HELP.callOut],
    ['transitive in', fmt(metrics.transitiveFanIn), METRIC_HELP.transitiveFanIn],
    ['transitive out', fmt(metrics.transitiveFanOut), METRIC_HELP.transitiveFanOut],
    ['depth', fmt(metrics.depth), METRIC_HELP.depth],
    ['height', fmt(metrics.height), METRIC_HELP.height],
    ['pageRank', metrics.pageRank.toFixed(4), METRIC_HELP.pageRank],
    ['betweenness', fmt(metrics.betweenness), METRIC_HELP.betweenness],
    ['instability', fmt(metrics.instability), METRIC_HELP.instability],
    ['cyclomatic', fmt(node.cyclomatic), METRIC_HELP.cyclomatic],
    ['IFC', fmt(metrics.ifc), METRIC_HELP.ifc],
    ['LOC', fmt(node.loc), METRIC_HELP.loc],
    ['params', fmt(node.params), METRIC_HELP.params],
    ['ignored results', pct(metrics.resultIgnoredRatio), METRIC_HELP.resultIgnoredRatio],
    ['constants used', fmt(metrics.constantsUsed), METRIC_HELP.constantsUsed],
    ['external out', fmt(metrics.externalFanOut), METRIC_HELP.externalFanOut],
    ['module refs', fmt(metrics.moduleRefs), METRIC_HELP.moduleRefs],
    ['hotspot', metrics.hotspot.toFixed(2), METRIC_HELP.hotspot],
  ];

  return (
    <section className="panel details" id="details">
      <div className="details-head">
        <div>
          <div className="kind-line">
            <span className={`kind kind-${node.kind}`}>{node.kind}</span>
            <span className="loc">{node.file}:{node.line}</span>
          </div>
          <h2>{node.qname}</h2>
        </div>
        <button className="button small" onClick={onClearSelection} title="Clear selection (Esc)">×</button>
      </div>
      <div className="flags">
        {node.exported ? <span className="badge">exported</span> : null}
        {node.isEntryLike ? <span className="badge success">entry</span> : null}
        {node.external ? <span className="badge">external</span> : null}
        {node.lazy ? <span className="badge" title="Discovered lazily via call hierarchy">lazy</span> : null}
        {node.synthetic ? <span className="badge" title="Implicit constructor">implicit</span> : null}
        {metrics.inCycle ? <span className="badge warning">cycle ×{metrics.sccSize}</span> : null}
        {metrics.deadCodeCandidate ? <span className="badge danger">dead-code candidate</span> : null}
      </div>
      <pre className="declaration">{node.declaration || '—'}</pre>
      <div className="hotspot-meter" title={METRIC_HELP.hotspot}>
        <span>hotspot</span>
        <div className="meter"><i style={{ width: `${Math.round(metrics.hotspot * 100)}%`, background: hotspotColor(metrics.hotspot) }} /></div>
        <b>{metrics.hotspot.toFixed(2)}</b>
      </div>
      <div className="metrics-grid">
        {rows.map(([label, value, help]) => (
          <div className="metric" key={label} title={help}>
            <span className="metric-label">{label}</span>
            <span className="metric-value">{value}</span>
          </div>
        ))}
      </div>
      <div className="relations">
        <RelationList title="Called by" edges={incoming.filter((edge) => edge.kind === 'call')} side="source" report={report} onSelect={onSelect} />
        <RelationList title="Calls" edges={outgoing.filter((edge) => edge.kind === 'call')} side="target" report={report} onSelect={onSelect} />
        <RelationList title="Uses constants" edges={outgoing.filter((edge) => edge.kind === 'data')} side="target" report={report} onSelect={onSelect} />
        <RelationList title="Used by" edges={incoming.filter((edge) => edge.kind === 'data')} side="source" report={report} onSelect={onSelect} />
      </div>
      <div className="detail-actions">
        <button className="button" onClick={() => onFocus(node.id)}>Focus connected slice</button>
        {focused ? <button className="button" onClick={onClearFocus}>Clear focus</button> : null}
      </div>
    </section>
  );
}

function hotspotColor(value: number): string {
  if (value >= 0.66) return 'var(--danger)';
  if (value >= 0.33) return 'var(--warning)';
  return 'var(--success)';
}
