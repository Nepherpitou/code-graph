import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from 'react';
import {
  getNodesBounds,
  getViewportForBounds,
  useReactFlow,
  type NodeMouseHandler,
} from '@xyflow/react';
import { toPng } from 'html-to-image';

import { fetchEntryPoints, fetchFocus, fetchImpact, fetchPath } from '../../lib/api';
import type {
  EntryPoint,
  FocusSlice,
  GraphEdge,
  GraphNode,
  GraphReport,
  GraphSlice,
  GraphSummary,
  PathResult,
} from '../../types';
import {
  defaultFilters,
  hiddenRelations,
  neighborhood,
  nodeSize,
  toFlowEdges,
  toFlowNodes,
  toGhostEdges,
  toGhostNodes,
  visibleGraph,
  type AnyFlowNode,
  type Filters,
  type FlowEdge,
} from './graph';
import { cancelLayout, computeLayout, type LayoutKind } from './layout';

/**
 * The readable zoom shows only a local window of a very large graph. Fitting
 * all nodes would land in an empty gap between separated components; choose
 * the densest bucket instead.
 */
function denseCenter(nodes: AnyFlowNode[]): { x: number; y: number } | null {
  if (nodes.length === 0) return null;
  const cell = 900;
  const buckets = new Map<string, { x: number; y: number; count: number }>();
  for (const node of nodes) {
    const x = node.position.x + Number(node.width ?? 0) / 2;
    const y = node.position.y + Number(node.height ?? 0) / 2;
    const key = `${Math.floor(x / cell)}:${Math.floor(y / cell)}`;
    const bucket = buckets.get(key) ?? { x: 0, y: 0, count: 0 };
    bucket.x += x;
    bucket.y += y;
    bucket.count += 1;
    buckets.set(key, bucket);
  }
  let best: { x: number; y: number; count: number } | null = null;
  for (const bucket of buckets.values()) {
    if (!best || bucket.count > best.count) best = bucket;
  }
  return best ? { x: best.x / best.count, y: best.y / best.count } : null;
}

export type GraphMode = 'focus' | 'path' | 'impact';

interface Input {
  report: GraphReport | null;
  snapshot: GraphSummary | null;
  setReport: Dispatch<SetStateAction<GraphReport | null>>;
  apiContext: { base: string; graphId: string } | null;
  setError: (message: string | null) => void;
  mode: GraphMode;
}

const FOCUS_DEPTH = 1;
const FOCUS_LIMIT = 180;

export interface GraphView {
  report: GraphReport | null;
  visible: { nodes: GraphNode[]; edges: GraphEdge[] };
  selectedNode: GraphNode | null;
  selectedId: string | null;
  setSelectedId: (id: string | null) => void;
  neighborIds: Set<string>;
  flowNodes: AnyFlowNode[];
  flowEdges: FlowEdge[];
  filters: Filters;
  setFilters: React.Dispatch<React.SetStateAction<Filters>>;
  searchText: string;
  setSearchText: (value: string) => void;
  layoutKind: LayoutKind;
  setLayoutKind: (kind: LayoutKind) => void;
  layoutNonce: number;
  relayout: () => void;
  layouting: boolean;
  focusIds: Set<string> | null;
  entryRoots: EntryPoint[];
  entryInfo: { totalRoots: number; depth: number; truncated: boolean } | null;
  openEntryPoints: () => Promise<void>;
  mergeSlice: (slice: GraphSlice, focus: Set<string> | null) => void;
  focusNode: (id: string) => void;
  clearFocus: () => void;
  clearSelection: () => void;
  selectAndCenter: (id: string) => void;
  path: PathResult | null;
  pathIndex: number;
  setPathIndex: (index: number) => void;
  findPath: (fromId: string, toId: string) => Promise<boolean>;
  showPathScope: () => void;
  clearRouteHighlight: () => void;
  clearPath: () => void;
  applyPath: (result: PathResult) => void;
  openNeighborhood: (node: GraphNode) => Promise<void>;
  showImpact: (node: GraphNode, direction: 'in' | 'out') => Promise<void>;
  resetGraphView: () => void;
  exportPng: () => Promise<void>;
  onNodeClick: NodeMouseHandler;
  onNodeDoubleClick: NodeMouseHandler;
}

export function useGraphView({
  report,
  snapshot,
  setReport,
  apiContext,
  setError,
  mode,
}: Input): GraphView {
  const reactFlow = useReactFlow();
  const [filters, setFilters] = useState<Filters>(() => defaultFilters());
  const [searchText, setSearchText] = useState('');
  const [layoutKind, setLayoutKind] = useState<LayoutKind>('layered');
  const [positions, setPositions] = useState<Record<string, { x: number; y: number }>>({});
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [focusIds, setFocusIds] = useState<Set<string> | null>(null);
  const [routeIds, setRouteIds] = useState<Set<string>>(new Set());
  const [path, setPath] = useState<PathResult | null>(null);
  const [pathIndex, setPathIndexState] = useState(0);
  const [entryRoots, setEntryRoots] = useState<EntryPoint[]>([]);
  const [entryInfo, setEntryInfo] = useState<{
    totalRoots: number;
    depth: number;
    truncated: boolean;
  } | null>(null);
  const [layouting, setLayouting] = useState(false);
  const [layoutNonce, setLayoutNonce] = useState(0);
  const lastLayoutConfig = useRef({ kind: '', nonce: -1, scope: '' });
  const cameraZoom = useRef<number | null>(null);
  const selectedIdRef = useRef<string | null>(null);
  selectedIdRef.current = selectedId;
  const centeredPositions = useRef<Record<string, { x: number; y: number }> | null>(null);

  const entryIds = useMemo(
    () => (entryRoots.length ? new Set(entryRoots.map((root) => root.id)) : null),
    [entryRoots],
  );

  const visible = useMemo(
    () => (report ? visibleGraph(report, filters, focusIds) : { nodes: [], edges: [] }),
    [report, filters, focusIds],
  );

  const selectedNode = useMemo(
    () => (selectedId ? report?.nodes.find((node) => node.id === selectedId) ?? null : null),
    [report, selectedId],
  );

  const neighborIds = useMemo(() => {
    const result = new Set<string>();
    if (!report || !selectedId) return result;
    result.add(selectedId);
    for (const edge of report.edges) {
      if (edge.source === selectedId) result.add(edge.target);
      if (edge.target === selectedId) result.add(edge.source);
    }
    return result;
  }, [report, selectedId]);

  const hidden = useMemo(
    () => hiddenRelations(visible.nodes, visible.edges),
    [visible],
  );

  const ghostNodes = useMemo(
    () => toGhostNodes(visible.nodes, hidden, positions),
    [visible.nodes, hidden, positions],
  );

  const flowNodes = useMemo<AnyFlowNode[]>(
    () => [
      ...toFlowNodes(visible.nodes, selectedId, neighborIds, entryIds, routeIds).map((node) => ({
        ...node,
        position: positions[node.id] ?? node.position,
      })),
      ...ghostNodes,
    ],
    [visible.nodes, selectedId, neighborIds, entryIds, routeIds, positions, ghostNodes],
  );

  const ghostEdges = useMemo(
    () => toGhostEdges(visible.nodes, hidden, positions),
    [visible.nodes, hidden, positions],
  );

  const flowEdges = useMemo<FlowEdge[]>(
    () => [
      ...toFlowEdges(visible.edges, selectedId, neighborIds, routeIds),
      ...ghostEdges,
    ],
    [visible.edges, selectedId, neighborIds, routeIds, ghostEdges],
  );

  const mergeSlice = useCallback((slice: GraphSlice, focus: Set<string> | null) => {
    // A focused slice is a new scope; a plain selection only adds that node's
    // immediate relations to the current entry-point view.
    setRouteIds(new Set());
    if (focus) {
      setEntryRoots([]);
      setEntryInfo(null);
    }
    setReport((current) => {
      const base = current ?? (snapshot ? {
        meta: snapshot.meta,
        nodes: [],
        edges: [],
        files: [],
        stats: snapshot.stats,
        top: snapshot.top,
        warnings: snapshot.warnings,
      } : null);
      if (!base) return current;
      const nodes = new Map(base.nodes.map((node) => [node.id, node]));
      for (const node of slice.nodes) nodes.set(node.id, node);
      const edges = new Map(base.edges.map((edge) => [edge.id, edge]));
      for (const edge of slice.edges) edges.set(edge.id, edge);
      return { ...base, nodes: [...nodes.values()], edges: [...edges.values()] };
    });
    if (focus) setFocusIds(focus);
  }, [setReport, snapshot]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setFilters((current) => (current.search === searchText ? current : { ...current, search: searchText }));
      if (searchText.trim()) {
        // A filtered graph is a new result set; keeping a selection outside it
        // would dim every match and leave an unrelated inspector open.
        setSelectedId(null);
        setFocusIds(null);
      }
    }, 180);
    return () => window.clearTimeout(timer);
  }, [searchText]);

  const layoutNodes = useMemo(
    () => visible.nodes.map((node) => ({ id: node.id, ...nodeSize(node) })),
    [visible.nodes],
  );
  const layoutScope = report?.meta.generatedAt ?? '';
  const viewKey = useMemo(
    () =>
      `${layoutScope}|${layoutKind}|${layoutNonce}|${layoutNodes.map((node) => node.id).join('|')}|${visible.edges
        .map((edge) => edge.id)
        .join('|')}`,
    [layoutScope, layoutKind, layoutNonce, layoutNodes, visible.edges],
  );

  useEffect(() => {
    if (layoutNodes.length === 0) {
      setPositions({});
      return;
    }
    let cancelled = false;
    setLayouting(true);
    const timer = window.setTimeout(async () => {
      try {
        const previous = lastLayoutConfig.current;
        const preserve = previous.kind === layoutKind
          && previous.nonce === layoutNonce
          && previous.scope === layoutScope;
        lastLayoutConfig.current = { kind: layoutKind, nonce: layoutNonce, scope: layoutScope };
        const next = await computeLayout(layoutNodes, visible.edges, layoutKind, layoutScope);
        if (cancelled || !next) return;
        setPositions((current) => {
          if (!preserve) return next;
          // Deep navigation only adds a focus slice. Keep already-placed nodes
          // exactly where they are; place only the newly revealed nodes.
          const anchorId = selectedIdRef.current && next[selectedIdRef.current]
            ? selectedIdRef.current
            : layoutNodes.find((node) => current[node.id] && next[node.id])?.id;
          const anchor = anchorId ? current[anchorId] : null;
          const anchorNext = anchorId ? next[anchorId] : null;
          const dx = anchor && anchorNext ? anchor.x - anchorNext.x : 0;
          const dy = anchor && anchorNext ? anchor.y - anchorNext.y : 0;
          const merged = { ...current };
          for (const [id, position] of Object.entries(next)) {
            if (!(id in merged)) merged[id] = { x: position.x + dx, y: position.y + dy };
          }
          return merged;
        });
        setLayouting(false);
      } catch (caught) {
        if (cancelled) return;
        setLayouting(false);
        setError(`Layout failed: ${(caught as Error).message}`);
      }
    }, 80);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      cancelLayout();
    };
  }, [viewKey, layoutScope]);

  const frameNodes = useCallback((nodes: AnyFlowNode[]) => {
    if (nodes.length < 2) return;
    const box = nodes.reduce((acc, node) => {
      const x = node.position.x;
      const y = node.position.y;
      const width = Number(node.width ?? 0);
      const height = Number(node.height ?? 0);
      return {
        minX: Math.min(acc.minX, x),
        minY: Math.min(acc.minY, y),
        maxX: Math.max(acc.maxX, x + width),
        maxY: Math.max(acc.maxY, y + height),
      };
    }, { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity });
    const parent = document.querySelector('.react-flow')?.getBoundingClientRect();
    if (!parent || !Number.isFinite(box.minX)) return;
    const padding = 56;
    const zoom = Math.min(
      (parent.width - padding * 2) / Math.max(1, box.maxX - box.minX),
      (parent.height - padding * 2) / Math.max(1, box.maxY - box.minY),
      1.15,
    );
    reactFlow.setCenter(
      (box.minX + box.maxX) / 2,
      (box.minY + box.maxY) / 2,
      { zoom: Math.max(0.25, zoom), duration: 0 },
    );
  }, [reactFlow]);

  useEffect(() => {
    if (Object.keys(positions).length === 0 || centeredPositions.current === positions) return;
    centeredPositions.current = positions;
    // Path mode fits the whole route; centering the endpoint would push the
    // entry and middle steps off-screen.
    if (mode === 'path' && path) return;

    const selected = selectedId
      ? flowNodes.find((node) => node.id === selectedId)
      : null;
    const rootId = entryRoots[0]?.id;
    const root = rootId ? flowNodes.find((node) => node.id === rootId) : null;
    const target = selected ?? root;
    const center = target
      ? {
          x: target.position.x + Number(target.width ?? 0) / 2,
          y: target.position.y + Number(target.height ?? 0) / 2,
        }
      : denseCenter(flowNodes);
    if (!center) return;
    void reactFlow.setCenter(center.x, center.y, {
      zoom: cameraZoom.current ?? reactFlow.getZoom(),
      duration: 0,
    });
  }, [positions, flowNodes, entryRoots, selectedId, reactFlow, mode, path]);

  // Path mode is about the whole route, not the endpoint under the cursor.
  useEffect(() => {
    if (mode !== 'path' || !path) return;
    const route = new Set(path.path);
    const routeNodes = flowNodes.filter((node) => route.has(node.id));
    if (routeNodes.length < 2) return;
    const timer = window.setTimeout(() => frameNodes(routeNodes), 120);
    return () => window.clearTimeout(timer);
  }, [mode, path, flowNodes, frameNodes]);

  const showFocusedSlice = useCallback((slice: FocusSlice, id: string) => {
    if (!snapshot) return;
    const focusedReport: GraphReport = {
      meta: snapshot.meta,
      nodes: slice.nodes,
      edges: slice.edges,
      files: [],
      stats: snapshot.stats,
      top: snapshot.top,
      warnings: snapshot.warnings,
    };
    // A focus selection is node + direct relations + the shortest entry→exit
    // route. The API slice also carries a wider context window; do not show it.
    const focus = neighborhood(focusedReport, id, FOCUS_DEPTH);
    for (const routeNode of slice.route) focus.add(routeNode);
    // Merge rather than replace: previously placed nodes keep their positions
    // when the user walks deeper through the call graph.
    mergeSlice(slice, focus);
    setEntryRoots([]);
    setEntryInfo(null);
    setPath(null);
    setPathIndexState(0);
    setRouteIds(new Set(slice.route));
    setSelectedId(id);
  }, [mergeSlice, snapshot, setEntryInfo, setEntryRoots, setSelectedId]);

  const selectAndCenter = useCallback((id: string) => {
    setSelectedId(id);
    // A node selection is a new exploration scope: reveal only its connected
    // slice so the node budget is spent on neighbours, not on unrelated entry
    // points that happen to be in the previous view.
    setSearchText('');
    setFilters((current) => (current.search ? { ...current, search: '' } : current));

    const node = flowNodes.find((entry) => entry.id === id);
    if (node) {
      const width = Number(node.width ?? 0);
      const height = Number(node.height ?? 0);
      const zoom = reactFlow.getZoom();
      cameraZoom.current = zoom;
      void reactFlow.setCenter(
        node.position.x + width / 2,
        node.position.y + height / 2,
        { zoom, duration: 300 },
      );
    }

    if (!apiContext || !snapshot) {
      // Local JSON report: filter the in-memory graph around the selection.
      if (!report) return;
      setEntryRoots([]);
      setEntryInfo(null);
      setRouteIds(new Set());
      setFocusIds(neighborhood(report, id, FOCUS_DEPTH));
      return;
    }

    // Direct neighbours, the shortest entry→selection→exit route, and the
    // surrounding two-hop context, with the same node budget as the initial view.
    void fetchFocus(apiContext.base, apiContext.graphId, id, FOCUS_LIMIT)
      .then((slice) => showFocusedSlice(slice, id))
      .catch((caught: Error) => setError(caught.message));
  }, [
    flowNodes,
    reactFlow,
    apiContext,
    snapshot,
    report,
    showFocusedSlice,
    setEntryInfo,
    setEntryRoots,
    setError,
    setFilters,
    setFocusIds,
    setSearchText,
  ]);

  const focusNode = useCallback((id: string) => {
    selectAndCenter(id);
  }, [selectAndCenter]);

  const clearFocus = useCallback(() => {
    setFocusIds(null);
    setSelectedId(null);
  }, []);

  const clearSelection = useCallback(() => {
    setSelectedId(null);
    reactFlow.setNodes((nodes) => nodes.map((node) => ({ ...node, selected: false })));
  }, [reactFlow]);

  const applyPath = useCallback((result: PathResult) => {
    if (!result.found) return;
    mergeSlice({ nodes: result.nodes, edges: result.edges }, new Set(result.path));
    setRouteIds(new Set(result.path));
    setPath(result);
    setPathIndexState(Math.max(0, result.path.length - 1));
    setSelectedId(result.path[result.path.length - 1] ?? null);
    setSearchText('');
    setFilters((current) => ({ ...current, search: '' }));
  }, [mergeSlice, setFilters, setSelectedId, setSearchText]);

  const findPath = useCallback(async (fromId: string, toId: string): Promise<boolean> => {
    if (!apiContext) {
      setError('Path needs the API server; open a graph.json report for focus and impact only.');
      return false;
    }
    try {
      const result = await fetchPath(apiContext.base, apiContext.graphId, fromId, toId, 16);
      if (!result.found) {
        setError('No call path between the selected endpoints.');
        return false;
      }
      applyPath(result);
      return true;
    } catch (caught) {
      setError(`Path failed: ${(caught as Error).message}`);
      return false;
    }
  }, [apiContext, applyPath, setError]);

  const setPathIndex = useCallback((index: number) => {
    if (!path) return;
    const next = Math.max(0, Math.min(path.path.length - 1, index));
    setPathIndexState(next);
    setSelectedId(path.path[next] ?? null);
    setRouteIds(new Set(path.path));
  }, [path, setSelectedId]);

  const showPathScope = useCallback(() => {
    if (!path) return;
    setFocusIds(new Set(path.path));
    setRouteIds(new Set(path.path));
    setSelectedId(path.path[pathIndex] ?? path.path[path.path.length - 1] ?? null);
    setEntryRoots([]);
    setEntryInfo(null);
  }, [path, pathIndex, setEntryInfo, setEntryRoots, setSelectedId]);

  const clearRouteHighlight = useCallback(() => {
    setRouteIds(new Set());
  }, []);

  const clearPath = useCallback(() => {
    setPath(null);
    setPathIndexState(0);
    setRouteIds(new Set());
    setFocusIds(null);
  }, []);

  const openNeighborhood = useCallback(async (node: GraphNode) => {
    selectAndCenter(node.id);
  }, [selectAndCenter]);

  const openEntryPoints = useCallback(async () => {
    if (!apiContext || !snapshot) return;
    try {
      const data = await fetchEntryPoints(apiContext.base, apiContext.graphId, 8, 2, 180);
      const warnings = data.truncated
        ? [
            ...snapshot.warnings,
            `entry graph limited to ${data.nodes.length} nodes across ${data.roots.length} roots`,
          ]
        : snapshot.warnings;
      setReport({
        meta: snapshot.meta,
        nodes: data.nodes,
        edges: data.edges,
        files: [],
        stats: snapshot.stats,
        top: snapshot.top,
        warnings,
      });
      setEntryRoots(data.roots);
      setEntryInfo({
        totalRoots: data.totalRoots,
        depth: data.depth,
        truncated: data.truncated,
      });
      setSelectedId(null);
      setFocusIds(null);
      setRouteIds(new Set());
      setPath(null);
      setPathIndexState(0);
      setFilters(defaultFilters());
      setSearchText('');
      setPositions({});
    } catch (caught) {
      setError(`Entry points failed: ${(caught as Error).message}`);
    }
  }, [apiContext, snapshot, setError, setReport]);

  const showImpact = useCallback(async (node: GraphNode, direction: 'in' | 'out') => {
    if (!apiContext) return;
    try {
      const slice = await fetchImpact(apiContext.base, apiContext.graphId, node.id, 2, direction);
      mergeSlice(slice, new Set(slice.nodes.map((sliceNode) => sliceNode.id)));
      setSelectedId(node.id);
    } catch (caught) {
      setError(`Impact failed: ${(caught as Error).message}`);
    }
  }, [apiContext, mergeSlice, setError, setSelectedId]);

  const exportPng = useCallback(async () => {
    if (flowNodes.length === 0) return;
    const bounds = getNodesBounds(flowNodes);
    const width = Math.min(6000, Math.max(1000, Math.ceil(bounds.width + 160)));
    const height = Math.min(6000, Math.max(700, Math.ceil(bounds.height + 160)));
    const viewport = getViewportForBounds(bounds, width, height, 0.2, 2, 0.1);
    const element = document.querySelector('.react-flow__viewport') as HTMLElement | null;
    if (!element) return;
    const background = getComputedStyle(document.documentElement).getPropertyValue('--canvas').trim() || '#0d1117';
    const dataUrl = await toPng(element, {
      backgroundColor: background,
      width,
      height,
      style: {
        width: `${width}px`,
        height: `${height}px`,
        transform: `translate(${viewport.x}px, ${viewport.y}px) scale(${viewport.zoom})`,
      },
    });
    const anchor = document.createElement('a');
    anchor.href = dataUrl;
    anchor.download = `code-graph-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')}.png`;
    anchor.click();
  }, [flowNodes]);

  const onNodeClick: NodeMouseHandler = (_event, node) => {
    if (mode === 'impact') {
      const target = report?.nodes.find((candidate) => candidate.id === node.id);
      if (target) void showImpact(target, 'in');
      return;
    }
    if (mode === 'path' && path) {
      const index = path.path.indexOf(node.id);
      if (index >= 0) {
        setPathIndex(index);
        return;
      }
      setSelectedId(node.id);
      return;
    }
    selectAndCenter(node.id);
  };
  const onNodeDoubleClick: NodeMouseHandler = (_event, node) => {
    focusNode(node.id);
  };
  const resetGraphView = useCallback(() => {
    setSelectedId(null);
    setFocusIds(null);
    setRouteIds(new Set());
    setPath(null);
    setPathIndexState(0);
    setEntryRoots([]);
    setEntryInfo(null);
    setFilters(defaultFilters());
    setSearchText('');
    setPositions({});
  }, []);

  return {
    report,
    visible,
    selectedNode,
    selectedId,
    setSelectedId,
    neighborIds,
    flowNodes,
    flowEdges,
    filters,
    setFilters,
    searchText,
    setSearchText,
    layoutKind,
    setLayoutKind,
    layoutNonce,
    relayout: () => setLayoutNonce((value) => value + 1),
    layouting,
    focusIds,
    entryRoots,
    entryInfo,
    openEntryPoints,
    mergeSlice,
    focusNode,
    clearFocus,
    clearSelection,
    selectAndCenter,
    path,
    pathIndex,
    setPathIndex,
    findPath,
    showPathScope,
    clearRouteHighlight,
    clearPath,
    applyPath,
    openNeighborhood,
    showImpact,
    resetGraphView,
    exportPng,
    onNodeClick,
    onNodeDoubleClick,
  };
}
