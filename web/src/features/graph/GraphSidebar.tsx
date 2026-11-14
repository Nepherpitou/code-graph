import type { GraphSummary } from '../../types';
import type { GraphView } from './useGraphView';
import { ServerPanel } from './ServerPanel';
import { FilterPanel } from './FilterPanel';
import { InsightsPanel } from './InsightsPanel';

interface Props {
  view: GraphView;
  apiContext: { base: string; graphId: string } | null;
  snapshot: GraphSummary | null;
  onError: (message: string) => void;
}

export function GraphSidebar({ view, apiContext, snapshot, onError }: Props) {
  const report = view.report;
  const panelReport = report ?? (snapshot ? {
    meta: snapshot.meta,
    nodes: [],
    edges: [],
    files: [],
    stats: snapshot.stats,
    top: snapshot.top,
    warnings: snapshot.warnings,
  } : null);

  return (
    <>
      {report && apiContext ? (
        <ServerPanel
          base={apiContext.base}
          graphId={apiContext.graphId}
          selectedId={view.selectedId}
          onExpand={(slice, focus) => {
            view.mergeSlice(slice, focus.size > 1 ? focus : null);
            if (focus.size === 1) view.setSelectedId([...focus][0]);
          }}
          onPath={view.applyPath}
          onError={onError}
        />
      ) : null}
      {report ? (
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
      ) : null}
      {panelReport ? (
        <InsightsPanel
          report={panelReport}
          onSelect={view.selectAndCenter}
          onFilterFile={(file) => view.setSearchText(file)}
        />
      ) : null}
    </>
  );
}
