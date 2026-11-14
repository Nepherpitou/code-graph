import type { GraphReport, TopEntry } from '../../types';

interface Props {
  report: GraphReport;
  onSelect: (id: string) => void;
  onFilterFile: (file: string) => void;
}

function pct(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}

function fmt(value: number, digits = 2): string {
  if (!Number.isFinite(value)) return '—';
  if (value === 0) return '0';
  if (Math.abs(value) >= 1000) return value.toFixed(0);
  return value.toFixed(digits).replace(/\.?0+$/, '');
}

function topFormat(value: number, metric: string): string {
  if (metric === 'pageRank') return value.toFixed(4);
  if (metric === 'hotspot') return value.toFixed(2);
  if (metric === 'ifc') return value.toFixed(0);
  return fmt(value);
}

function TopList({ title, rows, metric, onSelect }: { title: string; rows: TopEntry[]; metric: string; onSelect: (id: string) => void }) {
  if (rows.length === 0) return null;
  return (
    <details className="top-list">
      <summary>{title}</summary>
      {rows.slice(0, 8).map((row) => (
        <button key={row.id} className="top-row" onClick={() => onSelect(row.id)} title={row.file}>
          <span className="top-value">{topFormat(row.value, metric)}</span>
          <span className="top-name">{row.name}</span>
        </button>
      ))}
    </details>
  );
}

export function InsightsPanel({ report, onSelect, onFilterFile }: Props) {
  const { stats, top, files } = report;
  const rows: Array<[string, string, string]> = [
    ['density', fmt(stats.density, 4), 'call edges / possible directed pairs'],
    ['avg fan-in', fmt(stats.avgFanIn), 'mean distinct callers'],
    ['avg fan-out', fmt(stats.avgFanOut), 'mean distinct callees'],
    ['max fan-in', fmt(stats.maxFanIn), 'largest blast radius'],
    ['max fan-out', fmt(stats.maxFanOut), 'largest coupling'],
    ['cycles', `${stats.cycles} (${stats.nodesInCycles})`, 'SCCs larger than one node'],
    ['largest cycle', fmt(stats.largestCycle), 'nodes in the largest SCC'],
    ['dead code', fmt(stats.deadCode), 'never called, not exported'],
    ['isolated', fmt(stats.isolated), 'no callers and no callees'],
    ['ignored results', pct(stats.resultIgnoredRatio), 'call sites discarding values'],
    ['cross-file', pct(stats.crossFileRatio), 'call edges crossing files'],
    ['max depth', fmt(stats.maxDepth), 'longest chain from a source'],
    ['max height', fmt(stats.maxHeight), 'longest chain to a leaf'],
  ];

  return (
    <section className="panel insights">
      <h3>Graph insights</h3>
      <div className="stats-grid">
        {rows.map(([label, value, help]) => (
          <div className="stat" key={label} title={help}>
            <span className="stat-label">{label}</span>
            <span className="stat-value">{value}</span>
          </div>
        ))}
      </div>
      <div className="top-lists">
        <TopList title="Blast radius (fan-in)" rows={top.fanIn} metric="fanIn" onSelect={onSelect} />
        <TopList title="Coupling (fan-out)" rows={top.fanOut} metric="fanOut" onSelect={onSelect} />
        <TopList title="PageRank / CGBR" rows={top.pageRank} metric="pageRank" onSelect={onSelect} />
        <TopList title="Hotspots" rows={top.hotspot} metric="hotspot" onSelect={onSelect} />
        <TopList title="Cyclomatic complexity" rows={top.complexity} metric="complexity" onSelect={onSelect} />
        <TopList title="Information flow (IFC)" rows={top.ifc} metric="ifc" onSelect={onSelect} />
        {top.deadCode.length > 0 ? <TopList title="Dead-code candidates" rows={top.deadCode} metric="loc" onSelect={onSelect} /> : null}
      </div>
      <details className="files">
        <summary>Files ({files.length})</summary>
        <div className="file-list">
          {files.map((file) => (
            <button key={file.file} className="file-row" onClick={() => onFilterFile(file.file)} title={`Filter by ${file.file}`}>
              <span className="file-name">{file.file}</span>
              <span className="file-nums">{file.nodes} nodes · {file.callIn} in · {file.callOut} out</span>
            </button>
          ))}
        </div>
      </details>
      {report.warnings.length > 0 ? (
        <details className="warnings">
          <summary>{report.warnings.length} warning{report.warnings.length === 1 ? '' : 's'}</summary>
          <ul>{report.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>
        </details>
      ) : null}
    </section>
  );
}
