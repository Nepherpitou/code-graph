import type { GraphView } from './useGraphView';
import { FilterPanel } from './FilterPanel';
import { EntryPointsPanel } from './EntryPointsPanel';

interface Props {
  view: GraphView;
}

export function GraphSidebar({ view }: Props) {
  const report = view.report;

  return (
    <>
      {view.entryInfo ? (
        <details className="panel-fold">
          <summary>
            Entry points
            <span>{view.entryRoots.length} roots · depth {view.entryInfo.depth}</span>
          </summary>
          <EntryPointsPanel
            roots={view.entryRoots}
            totalRoots={view.entryInfo.totalRoots}
            depth={view.entryInfo.depth}
            truncated={view.entryInfo.truncated}
            onSelect={view.selectAndCenter}
          />
        </details>
      ) : null}

      {report ? (
        <details className="panel-fold">
          <summary>
            Filters &amp; layout
            <span>{view.visible.nodes.length} nodes in scope</span>
          </summary>
          <FilterPanel
            report={report}
            filters={view.filters}
            searchText={view.searchText}
            layoutKind={view.layoutKind}
            layouting={view.layouting}
            onChange={(patch) => view.setFilters((current) => ({ ...current, ...patch }))}
            onSearch={view.setSearchText}
            onLayout={view.setLayoutKind}
            onRelayout={view.relayout}
          />
        </details>
      ) : null}
    </>
  );
}
