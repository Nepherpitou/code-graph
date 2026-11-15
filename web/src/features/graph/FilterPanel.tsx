import type { GraphReport } from '../../types';
import type { Filters } from './graph';
import type { LayoutKind } from './layout';

const KINDS: Array<[string, string]> = [
  ['function', 'function'],
  ['method', 'method'],
  ['constructor', 'constructor'],
  ['constant', 'constant'],
  ['variable', 'variable'],
];

interface Props {
  report: GraphReport;
  filters: Filters;
  searchText: string;
  layoutKind: LayoutKind;
  layouting: boolean;
  onChange: (patch: Partial<Filters>) => void;
  onSearch: (value: string) => void;
  onLayout: (kind: LayoutKind) => void;
  onRelayout: () => void;
}

export function FilterPanel({
  report, filters, searchText, layoutKind, layouting, onChange, onSearch, onLayout, onRelayout,
}: Props) {
  const maxDegree = Math.max(1, ...report.nodes.map((node) => Math.max(node.metrics.fanIn, node.metrics.fanOut)));
  const toggleKind = (kind: string, checked: boolean): void => {
    const kinds = new Set(filters.kinds);
    if (checked) kinds.add(kind); else kinds.delete(kind);
    onChange({ kinds });
  };

  return (
    <section className="panel controls">
      <label className="field">
        <span>Search</span>
        <input
          id="search"
          type="search"
          value={searchText}
          placeholder="name, file…  (press /)"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => onSearch(event.target.value)}
        />
      </label>

      <div className="field-row">
        <label className="field">
          <span>Layout</span>
          <select id="layout" value={layoutKind} onChange={(event) => onLayout(event.target.value as LayoutKind)}>
            <option value="layered">Layered (ELK)</option>
            <option value="stress">Stress (force-ish)</option>
            <option value="grid">Grid (by file)</option>
          </select>
        </label>
        <button id="relayout" className="button relayout" onClick={onRelayout} disabled={layouting}>
          {layouting ? 'Layout…' : 'Re-layout'}
        </button>
      </div>

      <div className="field">
        <span>Node kinds</span>
        <div className="chips">
          {KINDS.map(([kind, label]) => (
            <label className="chip" key={kind}>
              <input
                type="checkbox"
                checked={filters.kinds.has(kind)}
                onChange={(event) => toggleKind(kind, event.target.checked)}
              />
              {label}
            </label>
          ))}
        </div>
      </div>

      <label className="field">
        <span>Min fan-in <output>{filters.minFanIn}</output></span>
        <input
          id="min-fan-in"
          type="range"
          min={0}
          max={maxDegree}
          step={1}
          value={filters.minFanIn}
          onChange={(event) => onChange({ minFanIn: Number(event.target.value) })}
        />
      </label>
      <label className="field">
        <span>Min fan-out <output>{filters.minFanOut}</output></span>
        <input
          id="min-fan-out"
          type="range"
          min={0}
          max={maxDegree}
          step={1}
          value={filters.minFanOut}
          onChange={(event) => onChange({ minFanOut: Number(event.target.value) })}
        />
      </label>

      <div className="checks">
        <label><input type="checkbox" checked={filters.hideDead} onChange={(event) => onChange({ hideDead: event.target.checked })} /> hide dead-code candidates</label>
        <label><input type="checkbox" checked={filters.hideExternal} onChange={(event) => onChange({ hideExternal: event.target.checked })} /> hide external nodes</label>
        <label><input type="checkbox" checked={filters.cyclesOnly} onChange={(event) => onChange({ cyclesOnly: event.target.checked })} /> only nodes in cycles</label>
        <label><input type="checkbox" checked={filters.hideIsolated} onChange={(event) => onChange({ hideIsolated: event.target.checked })} /> hide isolated nodes</label>
      </div>

      <div className="legend">
        <div className="legend-group">
          {KINDS.map(([kind, label]) => (
            <span className="legend-item" key={kind}><i className={`dot kind-${kind}`} />{label}</span>
          ))}
        </div>
        <div className="legend-group">
          <span className="legend-item"><i className="line" />call →</span>
          <span className="legend-item"><i className="line double accent" />result used ↔</span>
          <span className="legend-item"><i className="line dashed warning" />inferred (refs)</span>
          <span className="legend-item"><i className="line dashed success" />constant use ⇢</span>
          <span className="legend-item" title="shortest entry → exit route through the focused node">
            <i className="line success" />execution route
          </span>
          <span className="legend-item" title="connections outside the current slice">
            <i className="line dashed ghost" />hidden relations
          </span>
        </div>
      </div>
    </section>
  );
}
