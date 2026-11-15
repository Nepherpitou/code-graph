import { useCallback, useEffect, useState } from 'react';

import { fetchFileSlice } from './lib/api';
import type { GraphNode, OverviewNode } from './types';
import { DirectoryScreen } from './features/directories/DirectoryScreen';
import { FileScreen } from './features/files/FileScreen';
import { NodeTable } from './features/nodes/NodeTable';
import { NodeContextMenu, type ContextAction } from './features/nodes/NodeContextMenu';
import { GraphCanvas } from './features/graph/GraphCanvas';
import { GraphSidebar } from './features/graph/GraphSidebar';
import { DetailsPanel } from './features/graph/DetailsPanel';
import { useGraphView } from './features/graph/useGraphView';
import { useSnapshot } from './features/snapshot/useSnapshot';
import { shortRoot } from './shared/format';

type Screen = 'directories' | 'files' | 'graph';

export function App() {
  const state = useSnapshot();
  const graph = useGraphView({
    report: state.report,
    snapshot: state.snapshot,
    setReport: state.setReport,
    apiContext: state.apiContext,
    setError: state.setError,
  });
  const [screen, setScreen] = useState<Screen>('graph');
  const [directory, setDirectory] = useState<OverviewNode | null>(null);
  const [scopeLabel, setScopeLabel] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{ node: GraphNode; x: number; y: number } | null>(null);

  const openEntryPoints = useCallback(async () => {
    setDirectory(null);
    setScopeLabel('Entry points');
    setScreen('graph');
    await graph.openEntryPoints();
  }, [graph.openEntryPoints]);

  useEffect(() => {
    if (!state.snapshot || !state.apiContext) return;
    setDirectory(null);
    setScopeLabel('Entry points');
    setScreen('graph');
    void graph.openEntryPoints();
    // Load only when the snapshot identity changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.snapshot?.id, state.apiContext?.graphId]);

  const openDirectory = useCallback(async (next: OverviewNode) => {
    setDirectory(next);
    setScopeLabel(next.path);
    setScreen('files');
    const files = await state.loadFileOverview();
    if (!files) {
      setScreen('directories');
      setScopeLabel(null);
      setDirectory(null);
    }
  }, [state.loadFileOverview]);

  const openFileSlice = useCallback(async (path: string) => {
    if (!state.apiContext || !state.snapshot) return;
    try {
      const slice = await fetchFileSlice(
        state.apiContext.base,
        state.apiContext.graphId,
        path,
        'file',
        300,
      );
      const warnings = slice.truncated
        ? [...state.snapshot.warnings, `slice limited to ${slice.nodes.length} of ${slice.total} nodes`]
        : state.snapshot.warnings;
      state.setReport({
        meta: state.snapshot.meta,
        nodes: slice.nodes,
        edges: slice.edges,
        files: [],
        stats: state.snapshot.stats,
        top: state.snapshot.top,
        warnings,
      });
      setScopeLabel(slice.truncated ? `${path} · ${slice.nodes.length}/${slice.total}` : path);
      graph.resetGraphView();
      setScreen('graph');
    } catch (caught) {
      state.setError(`File slice failed: ${(caught as Error).message}`);
    }
  }, [state.apiContext, state.snapshot, state.setError, state.setReport, graph.resetGraphView]);

  const openFile = useCallback((file: OverviewNode) => {
    void openFileSlice(file.path);
  }, [openFileSlice]);

  const openNode = useCallback(async (node: GraphNode) => {
    await graph.openNeighborhood(node);
    setScopeLabel(node.qname);
    setScreen('graph');
  }, [graph.openNeighborhood]);

  const back = useCallback(() => {
    graph.resetGraphView();
    state.setReport(null);
    if (screen === 'graph') {
      if (directory && state.fileOverview) {
        setScreen('files');
        setScopeLabel(directory.path);
      } else {
        setScreen('directories');
        setScopeLabel(null);
      }
      return;
    }
    setScreen('directories');
    setDirectory(null);
    setScopeLabel(null);
  }, [screen, directory, state.fileOverview, state.setReport, graph.resetGraphView]);

  const backLabel = screen === 'graph' && directory && state.fileOverview ? 'Files' : 'Directories';
  const meta = state.snapshot?.meta ?? state.report?.meta;
  const stats = state.snapshot?.stats ?? state.report?.stats;
  const refreshing = state.analysisStatus?.state === 'running';
  const canRefresh = Boolean(state.apiContext && state.analysisStatus?.canAnalyze);
  const menuActions: ContextAction[] = contextMenu ? [
    { label: 'Focus connected slice', onSelect: () => graph.focusNode(contextMenu.node.id) },
    { label: 'Show callers', onSelect: () => void graph.showImpact(contextMenu.node, 'in') },
    { label: 'Show callees', onSelect: () => void graph.showImpact(contextMenu.node, 'out') },
    { label: 'Open containing file', onSelect: () => void openFileSlice(contextMenu.node.file) },
    {
      label: 'Copy node ID',
      onSelect: () => void navigator.clipboard?.writeText(contextMenu.node.id),
    },
    {
      label: 'Copy qualified name',
      onSelect: () => void navigator.clipboard?.writeText(contextMenu.node.qname),
    },
  ] : [];

  return (
    <div className="app">
      <header>
        <div className="brand">
          <span className="logo">◉</span>
          <span>code-graph</span>
          <span className="tag">LSP call graph</span>
        </div>
        <div className="meta" title={meta ? `${meta.server} · generated ${meta.generatedAt}` : undefined}>
          {meta && stats ? (
            <>
              <span className="root">{shortRoot(meta.root, 56)}</span>
              <span className="dot">·</span>
              <span className="lang">{meta.language}</span>
              <span className="dot">·</span>
              <span className="badge">{stats.nodes} nodes</span>
              <span className="badge">{stats.functions} callables</span>
              <span className="badge">{stats.constants} constants</span>
              <span className="badge">{stats.edges} call edges</span>
              <span className="badge">{stats.dataEdges} data edges</span>
              <span className="badge">{meta.files} files</span>
              <span className="badge">{meta.durationMs} ms</span>
              {state.apiContext ? <span className="badge accent">API #{state.apiContext.graphId}</span> : null}
              {scopeLabel ? <span className="badge">{scopeLabel}</span> : null}
              {meta.truncated ? <span className="badge danger">truncated</span> : null}
            </>
          ) : (
            <span className="muted">
              {state.loading && state.analysisStatus?.state === 'running'
                ? `${state.analysisStatus.phase}: ${state.analysisStatus.message}`
                : state.loading
                  ? 'Loading graph…'
                  : state.error ?? 'No graph loaded'}
            </span>
          )}
        </div>
        <div className="actions">
          {canRefresh ? (
            <button
              className="button"
              onClick={() => void state.refreshAnalysis()}
              disabled={refreshing}
              title="Re-read the project from disk and store a new snapshot"
            >
              {refreshing ? 'Refreshing…' : 'Refresh'}
            </button>
          ) : null}
          {state.apiContext ? (
            <button className="button" onClick={() => void openEntryPoints()}>Top entries</button>
          ) : null}
          {screen !== 'directories' && state.overview ? (
            <button className="button" onClick={back}>{backLabel}</button>
          ) : null}
          <label className="button">
            Open JSON
            <input
              type="file"
              accept="application/json,.json"
              hidden
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (!file) return;
                state.readFile(file);
                setScreen('graph');
                setDirectory(null);
                setScopeLabel(null);
              }}
            />
          </label>
          <button
            className="button"
            onClick={() => void graph.exportPng()}
            disabled={screen !== 'graph'}
          >
            PNG
          </button>
        </div>
      </header>

      <aside className="sidebar left">
        {state.allNodes.length > 0 ? (
          <NodeTable
            nodes={state.allNodes}
            total={stats?.nodes ?? state.allNodes.length}
            onOpen={(node) => void openNode(node)}
          />
        ) : null}
        {screen === 'graph' ? (
          <GraphSidebar
            view={graph}
            apiContext={state.apiContext}
            snapshot={state.snapshot}
            onError={(message) => state.setError(message)}
          />
        ) : null}
      </aside>

      <main className="canvas">
        {screen === 'directories' && state.overview ? (
          <DirectoryScreen
            overview={state.overview}
            totalNodes={stats?.nodes ?? state.allNodes.length}
            onOpen={(next) => void openDirectory(next)}
          />
        ) : screen === 'files' && state.fileOverview && directory ? (
          <FileScreen overview={state.fileOverview} directory={directory.path} onOpen={openFile} />
        ) : screen === 'graph' && state.report ? (
          <GraphCanvas
            view={graph}
            onContextMenu={(node, x, y) => setContextMenu({ node, x, y })}
          />
        ) : state.analysisStatus?.state === 'running' ? (
          <div className="empty">
            <h2>Analyzing…</h2>
            <p>{state.analysisStatus.message}</p>
            <div className="analysis-progress"><i /></div>
            <p className="muted">phase: {state.analysisStatus.phase}</p>
          </div>
        ) : (
          <div className="empty">
            <h2>{state.error ? 'Graph unavailable' : 'Loading graph…'}</h2>
            {state.error ? <p className="error-text">{state.error}</p> : null}
            <p>Build a graph from a codebase, then drop the JSON here:</p>
            <pre>cargo run -- analyze /path/to/repo -o web/public/graph.json</pre>
            <p>Or run <code>code-graph up</code> and keep this tab open.</p>
          </div>
        )}
      </main>

      <aside className="sidebar right">
        {screen === 'graph' && state.report ? (
          <DetailsPanel
            report={state.report}
            node={graph.selectedNode}
            onSelect={graph.selectAndCenter}
            onFocus={graph.focusNode}
            onClearFocus={graph.clearFocus}
            onClearSelection={graph.clearSelection}
            focused={graph.focusIds !== null}
          />
        ) : (
          <section className="panel">
            <h3>{screen === 'files' ? 'Files' : 'Directories'}</h3>
            <p className="muted">
              {screen === 'files'
                ? 'Pick a file to open its function graph.'
                : 'Pick a directory to browse its files.'}
            </p>
          </section>
        )}
      </aside>

      {contextMenu ? (
        <NodeContextMenu
          node={contextMenu.node}
          x={contextMenu.x}
          y={contextMenu.y}
          actions={menuActions}
          onClose={() => setContextMenu(null)}
        />
      ) : null}
    </div>
  );
}
