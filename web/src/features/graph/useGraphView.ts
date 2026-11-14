import {
  useCallback,
  useEffect,
  useMemo,
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

import { fetchImpact, fetchSubgraph } from '../../lib/api';
import type {
  GraphEdge,
  GraphNode,
  GraphReport,
  GraphSlice,
  GraphSummary,
  PathResult,
} from '../../types';
import {
  defaultFilters,
  neighborhood,
  nodeSize,
  toFlowEdges,
  toFlowNodes,
  visibleGraph,
  type Filters,
  type FlowEdge,
  type FlowNode,
} from './graph';
import { computeLayout, type LayoutKind } from './layout';

interface Input {
  report: GraphReport | null;
  snapshot: GraphSummary | null;
  setReport: Dispatch<SetStateAction<GraphReport | null>>;
  apiContext: { base: string; graphId: string } | null;
  setError: (message: string | null) => void;
}

export interface GraphView {
  report: GraphReport | null;
  visible: { nodes: GraphNode[]; edges: GraphEdge[] };
  selectedNode: GraphNode | null;
  selectedId: string | null;
  setSelectedId: (id: string | null) => void;
  neighborIds: Set<string>;
  flowNodes: FlowNode[];
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
  mergeSlice: (slice: GraphSlice, focus: Set<string> | null) => void;
  focusNode: (id: string) => void;
  clearFocus: () => void;
  clearSelection: () => void;
  selectAndCenter: (id: string) => void;
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
}: Input): GraphView {
  const reactFlow = useReactFlow();
  const [filters, setFilters] = useState<Filters>(() => defaultFilters());
  const [searchText, setSearchText] = useState('');
  const [layoutKind, setLayoutKind] = useState<LayoutKind>('layered');
  const [positions, setPositions] = useState<Record<string, { x: number; y: number }>>({});
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [focusIds, setFocusIds] = useState<Set<string> | null>(null);
  const [layouting, setLayouting] = useState(false);
  const [layoutNonce, setLayoutNonce] = useState(0);

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

  const flowNodes = useMemo<FlowNode[]>(
    () => toFlowNodes(visible.nodes, selectedId, neighborIds).map((node) => ({
      ...node,
      position: positions[node.id] ?? node.position,
    })),
    [visible.nodes, selectedId, neighborIds, positions],
  );

  const flowEdges = useMemo(
    () => toFlowEdges(visible.edges, selectedId, neighborIds),
    [visible.edges, selectedId, neighborIds],
  );

  const mergeSlice = useCallback((slice: GraphSlice, focus: Set<string> | null) => {
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
    }, 180);
    return () => window.clearTimeout(timer);
  }, [searchText]);

  const layoutNodes = useMemo(
    () => visible.nodes.map((node) => ({ id: node.id, ...nodeSize(node) })),
    [visible.nodes],
  );
  const viewKey = useMemo(
    () => `${layoutKind}|${layoutNonce}|${layoutNodes.map((node) => node.id).join('|')}`,
    [layoutKind, layoutNonce, layoutNodes],
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
        const next = await computeLayout(layoutNodes, visible.edges, layoutKind);
        if (cancelled) return;
        setPositions(next);
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
    };
  }, [viewKey]);

  useEffect(() => {
    if (layouting || Object.keys(positions).length === 0) return;
    const timer = window.setTimeout(() => {
      void reactFlow.fitView({ padding: 0.12, duration: 200, minZoom: 0.4 });
    }, 60);
    return () => window.clearTimeout(timer);
  }, [positions, layouting, reactFlow]);

  const selectAndCenter = useCallback((id: string) => {
    setSelectedId(id);
    const node = flowNodes.find((entry) => entry.id === id);
    if (node) {
      const width = Number(node.width ?? 0);
      const height = Number(node.height ?? 0);
      void reactFlow.setCenter(
        node.position.x + width / 2,
        node.position.y + height / 2,
        { zoom: Math.max(reactFlow.getZoom(), 0.7), duration: 350 },
      );
      return;
    }
    if (apiContext) {
      void fetchSubgraph(apiContext.base, apiContext.graphId, [id], 1, 300)
        .then((slice) => mergeSlice(slice, new Set(slice.nodes.map((node) => node.id))))
        .catch((caught: Error) => setError(caught.message));
    }
  }, [flowNodes, reactFlow, apiContext, mergeSlice, setError]);

  const focusNode = useCallback((id: string) => {
    if (!report) return;
    setSearchText('');
    setFilters((current) => ({ ...current, search: '' }));
    setFocusIds(neighborhood(report, id, 2));
    setSelectedId(id);
  }, [report, setFilters, setFocusIds, setSelectedId, setSearchText]);

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
    setSelectedId(result.path[result.path.length - 1] ?? null);
    setSearchText('');
    setFilters((current) => ({ ...current, search: '' }));
  }, [mergeSlice, setFilters, setSelectedId, setSearchText]);

  const openNeighborhood = useCallback(async (node: GraphNode) => {
    if (!apiContext) {
      if (!report) return;
      setSelectedId(node.id);
      setFocusIds(neighborhood(report, node.id, 1));
      return;
    }
    try {
      const slice = await fetchSubgraph(apiContext.base, apiContext.graphId, [node.id], 1, 300);
      mergeSlice(slice, new Set(slice.nodes.map((sliceNode) => sliceNode.id)));
      setSelectedId(node.id);
    } catch (caught) {
      setError(`Neighborhood failed: ${(caught as Error).message}`);
    }
  }, [apiContext, report, mergeSlice, setError, setFocusIds, setSelectedId]);

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
    setSelectedId(node.id);
  };
  const onNodeDoubleClick: NodeMouseHandler = (_event, node) => {
    focusNode(node.id);
  };
  const resetGraphView = useCallback(() => {
    setSelectedId(null);
    setFocusIds(null);
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
    mergeSlice,
    focusNode,
    clearFocus,
    clearSelection,
    selectAndCenter,
    applyPath,
    openNeighborhood,
    showImpact,
    resetGraphView,
    exportPng,
    onNodeClick,
    onNodeDoubleClick,
  };
}
