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
  type ReactFlowInstance,
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
  MIN_READABLE_ZOOM,
  neighborhood,
  nodeSize,
  toFlowEdges,
  toFlowNodes,
  visibleGraph,
  type Filters,
  type FlowEdge,
  type FlowNode,
} from './graph';
import { cancelLayout, computeLayout, type LayoutKind } from './layout';

/**
 * The readable zoom shows only a local window of a very large graph. Fitting
 * all nodes would land in an empty gap between separated components; choose
 * the densest bucket instead.
 */
interface PositionedNode {
  id: string;
  position: { x: number; y: number };
  width?: number | string | null;
  height?: number | string | null;
}

function denseCenter(nodes: PositionedNode[]): { x: number; y: number } | null {
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

interface CompactLayout {
  positions: Record<string, { x: number; y: number }>;
  rowHeight: number;
}

function directedDistances(
  edges: GraphEdge[],
  rootId: string,
  direction: 'in' | 'out',
): Map<string, number> {
  const adjacency = new Map<string, string[]>();
  for (const edge of edges) {
    const from = direction === 'in' ? edge.target : edge.source;
    const to = direction === 'in' ? edge.source : edge.target;
    const list = adjacency.get(from) ?? [];
    list.push(to);
    adjacency.set(from, list);
  }
  const distances = new Map<string, number>([[rootId, 0]]);
  const queue = [rootId];
  for (let head = 0; head < queue.length; head += 1) {
    const id = queue[head];
    const distance = distances.get(id)!;
    for (const next of adjacency.get(id) ?? []) {
      if (distances.has(next)) continue;
      distances.set(next, distance + 1);
      queue.push(next);
    }
  }
  return distances;
}

/** Focus mode grows outward by call direction: callers left, callees right. */
function focusLayered(nodes: GraphNode[], edges: GraphEdge[], rootId: string): CompactLayout {
  const nodeById = new Map(nodes.map((node) => [node.id, node]));
  if (!nodeById.has(rootId)) return compactLayered(nodes, edges, [rootId]);
  const sizes = new Map(nodes.map((node) => [node.id, nodeSize(node)]));
  const maxWidth = Math.max(180, ...nodes.map((node) => sizes.get(node.id)!.width));
  const maxHeight = Math.max(72, ...nodes.map((node) => sizes.get(node.id)!.height));
  const columnWidth = maxWidth + 140;
  const rowHeight = maxHeight + 56;
  const incoming = directedDistances(edges, rootId, 'in');
  const outgoing = directedDistances(edges, rootId, 'out');
  const incomingIds = [...incoming.keys()]
    .filter((id) => id !== rootId && nodeById.has(id) && !outgoing.has(id))
    .sort((left, right) =>
      incoming.get(left)! - incoming.get(right)!
      || nodeById.get(left)!.qname.localeCompare(nodeById.get(right)!.qname));
  const outgoingIds = [...outgoing.keys()]
    .filter((id) => id !== rootId && nodeById.has(id) && !incoming.has(id))
    .sort((left, right) =>
      outgoing.get(left)! - outgoing.get(right)!
      || nodeById.get(left)!.qname.localeCompare(nodeById.get(right)!.qname));
  const positions: Record<string, { x: number; y: number }> = {
    [rootId]: { x: 0, y: 0 },
  };
  const layersFor = (ids: string[], distances: Map<string, number>): [number, string[]][] => {
    const layers = new Map<number, string[]>();
    for (const id of ids) {
      const distance = Math.max(1, distances.get(id) ?? 1);
      const layer = layers.get(distance) ?? [];
      layer.push(id);
      layers.set(distance, layer);
    }
    return [...layers.entries()].sort(([left], [right]) => left - right);
  };
  let maxRows = 1;
  const place = (layers: [number, string[]][], direction: -1 | 1): void => {
    for (const [distance, ids] of layers) {
      ids.sort((left, right) => nodeById.get(left)!.qname.localeCompare(nodeById.get(right)!.qname));
      maxRows = Math.max(maxRows, ids.length);
      ids.forEach((id, index) => {
        positions[id] = {
          x: direction * distance * columnWidth,
          y: (index - (ids.length - 1) / 2) * rowHeight,
        };
      });
    }
  };
  place(layersFor(incomingIds, incoming), -1);
  place(layersFor(outgoingIds, outgoing), 1);
  const leftover = nodes
    .filter((node) => !positions[node.id])
    .map((node) => node.id)
    .sort((left, right) => nodeById.get(left)!.qname.localeCompare(nodeById.get(right)!.qname));
  const contextY = (maxRows / 2 + 1.5) * rowHeight;
  leftover.forEach((id, index) => {
    positions[id] = {
      x: (index % 3) * columnWidth,
      y: contextY + Math.floor(index / 3) * rowHeight,
    };
  });
  return { positions, rowHeight };
}

interface Neighbor {
  id: string;
  delta: number;
}

function layoutSpacing(nodes: GraphNode[]): { columnWidth: number; rowHeight: number } {
  const sizes = new Map(nodes.map((node) => [node.id, nodeSize(node)]));
  const maxWidth = Math.max(180, ...nodes.map((node) => sizes.get(node.id)!.width));
  const maxHeight = Math.max(72, ...nodes.map((node) => sizes.get(node.id)!.height));
  return { columnWidth: maxWidth + 140, rowHeight: maxHeight + 56 };
}

function buildNeighbors(
  nodeById: Map<string, GraphNode>,
  edges: GraphEdge[],
): Map<string, Neighbor[]> {
  const neighbors = new Map<string, Neighbor[]>();
  for (const edge of edges) {
    if (!nodeById.has(edge.source) || !nodeById.has(edge.target)) continue;
    const source = neighbors.get(edge.source) ?? [];
    source.push({ id: edge.target, delta: 1 });
    neighbors.set(edge.source, source);
    const target = neighbors.get(edge.target) ?? [];
    target.push({ id: edge.source, delta: -1 });
    neighbors.set(edge.target, target);
  }
  return neighbors;
}

function collectRootLevels(
  edges: GraphEdge[],
  rootSet: Set<string>,
): { first: Set<string>; second: Set<string> } {
  const first = new Set<string>();
  for (const edge of edges) {
    if (rootSet.has(edge.source) && !rootSet.has(edge.target)) first.add(edge.target);
  }
  const second = new Set<string>();
  for (const edge of edges) {
    if (first.has(edge.source) && !rootSet.has(edge.target) && !first.has(edge.target)) {
      second.add(edge.target);
    }
  }
  return { first, second };
}

function idsWhere(nodes: GraphNode[], predicate: (node: GraphNode) => boolean): string[] {
  return nodes.filter(predicate).map((node) => node.id);
}

function rootBandLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  roots: string[],
  columnWidth: number,
  rowHeight: number,
): Record<string, { x: number; y: number }> | null {
  if (roots.length < 2) return null;
  // Entry overview: roots are peers. Put every root in a top band, then the
  // first-level calls collected from all roots, then the second-level calls
  // if the node budget had room. Grid bands keep fifty roots visible without
  // turning the overview into a single endless column.
  const rootSet = new Set(roots);
  const { first, second } = collectRootLevels(edges, rootSet);
  const bands = [
    roots,
    idsWhere(nodes, (node) => first.has(node.id)),
    idsWhere(nodes, (node) => second.has(node.id)),
    idsWhere(nodes, (node) => !rootSet.has(node.id) && !first.has(node.id) && !second.has(node.id)),
  ];
  const positions: Record<string, { x: number; y: number }> = {};
  const columns = 10;
  let rowOffset = 0;
  for (const band of bands) {
    band.forEach((id, index) => {
      positions[id] = {
        x: (index % columns) * columnWidth,
        y: (rowOffset + Math.floor(index / columns)) * rowHeight,
      };
    });
    if (band.length > 0) rowOffset += Math.ceil(band.length / columns) + 1;
  }
  return positions;
}

function assignColumns(
  nodes: GraphNode[],
  roots: string[],
  neighbors: Map<string, Neighbor[]>,
  nodeById: Map<string, GraphNode>,
): Map<string, number> {
  // Columns are relative to the focused node, not global depth: sibling
  // callees must share a column even when their global depths differ.
  const columnById = new Map<string, number>();
  const queue: string[] = [];
  for (const rootId of roots) {
    columnById.set(rootId, 0);
    queue.push(rootId);
  }
  for (let head = 0; head < queue.length; head += 1) {
    const id = queue[head];
    const column = columnById.get(id)!;
    for (const next of neighbors.get(id) ?? []) {
      if (columnById.has(next.id)) continue;
      columnById.set(next.id, column + next.delta);
      queue.push(next.id);
    }
  }
  const rootDepths = roots
    .map((id) => nodeById.get(id)!.metrics.depth)
    .filter((depth) => Number.isFinite(depth))
    .map((depth) => Math.round(depth));
  const rootDepth = rootDepths.length > 0 ? Math.min(...rootDepths) : null;
  for (const node of nodes) {
    if (columnById.has(node.id)) continue;
    const depth = Number.isFinite(node.metrics.depth) ? Math.round(node.metrics.depth) : 0;
    columnById.set(node.id, rootDepth !== null ? depth - rootDepth : depth);
  }
  return columnById;
}

function parentRow(
  nodeId: string,
  incoming: Map<string, string[]>,
  rowById: Map<string, number>,
): number {
  const rows = (incoming.get(nodeId) ?? [])
    .map((id) => rowById.get(id))
    .filter((row): row is number => row !== undefined);
  return rows.length > 0 ? Math.min(...rows) : Infinity;
}

function compareScores(leftScore: number, rightScore: number): number {
  if (Number.isFinite(leftScore) !== Number.isFinite(rightScore)) {
    return Number.isFinite(leftScore) ? -1 : 1;
  }
  if (Number.isFinite(leftScore) && leftScore !== rightScore) return leftScore - rightScore;
  return 0;
}

function compareTieBreak(left: GraphNode, right: GraphNode): number {
  return Number(right.isEntryLike) - Number(left.isEntryLike)
    || left.file.localeCompare(right.file)
    || left.line - right.line
    || left.qname.localeCompare(right.qname)
    || left.id.localeCompare(right.id);
}

function compareLayerNodes(
  left: GraphNode,
  right: GraphNode,
  score: Map<string, number>,
): number {
  const scoreOrder = compareScores(score.get(left.id) ?? Infinity, score.get(right.id) ?? Infinity);
  return scoreOrder !== 0 ? scoreOrder : compareTieBreak(left, right);
}

function sortLayer(
  layerNodes: GraphNode[],
  incoming: Map<string, string[]>,
  rowById: Map<string, number>,
): void {
  const score = new Map<string, number>();
  for (const node of layerNodes) score.set(node.id, parentRow(node.id, incoming, rowById));
  layerNodes.sort((left, right) => compareLayerNodes(left, right, score));
}

function packLayerColumns(
  nodes: GraphNode[],
  columnById: Map<string, number>,
  incoming: Map<string, string[]>,
  columnWidth: number,
  rowHeight: number,
): Record<string, { x: number; y: number }> {
  const layers = new Map<number, GraphNode[]>();
  for (const node of nodes) {
    const column = columnById.get(node.id)!;
    const bucket = layers.get(column) ?? [];
    bucket.push(node);
    layers.set(column, bucket);
  }
  const positions: Record<string, { x: number; y: number }> = {};
  const rowById = new Map<string, number>();
  const columns = [...layers.keys()].sort((left, right) => left - right);
  for (const column of columns) {
    const layerNodes = layers.get(column) ?? [];
    sortLayer(layerNodes, incoming, rowById);
    layerNodes.forEach((node, row) => {
      positions[node.id] = { x: column * columnWidth, y: row * rowHeight };
      rowById.set(node.id, row);
    });
  }
  return positions;
}

function incomingEdges(edges: GraphEdge[]): Map<string, string[]> {
  const incoming = new Map<string, string[]>();
  for (const edge of edges) {
    const sources = incoming.get(edge.target) ?? [];
    sources.push(edge.source);
    incoming.set(edge.target, sources);
  }
  return incoming;
}

function compactLayered(nodes: GraphNode[], edges: GraphEdge[], rootIds: string[]): CompactLayout {
  if (nodes.length === 0) return { positions: {}, rowHeight: 0 };
  const { columnWidth, rowHeight } = layoutSpacing(nodes);
  const nodeById = new Map(nodes.map((node) => [node.id, node]));
  const roots = [...new Set(rootIds)].filter((id) => nodeById.has(id));
  const bands = rootBandLayout(nodes, edges, roots, columnWidth, rowHeight);
  if (bands) return { positions: bands, rowHeight };
  const neighbors = buildNeighbors(nodeById, edges);
  const columnById = assignColumns(nodes, roots, neighbors, nodeById);
  return {
    positions: packLayerColumns(nodes, columnById, incomingEdges(edges), columnWidth, rowHeight),
    rowHeight,
  };
}

function layeredPositionsForView(
  nodes: GraphNode[],
  edges: GraphEdge[],
  selectedId: string | null,
  roots: string[],
  focusIds: Set<string> | null,
  center: { x: number; y: number },
): Record<string, { x: number; y: number }> {
  const local = selectedId && focusIds !== null
    ? focusLayered(nodes, edges, selectedId)
    : compactLayered(nodes, edges, roots);
  const selectedNode = selectedId ? nodes.find((node) => node.id === selectedId) : null;
  const anchor = selectedNode
    ? {
        x: center.x - nodeSize(selectedNode).width / 2,
        y: center.y - nodeSize(selectedNode).height / 2,
      }
    : center;
  const localAnchorId = selectedId && local.positions[selectedId]
    ? selectedId
    : roots.find((id) => local.positions[id]) ?? nodes[0]?.id;
  const localAnchor = localAnchorId ? local.positions[localAnchorId] : null;
  const dx = localAnchor ? anchor.x - localAnchor.x : 0;
  const dy = localAnchor ? anchor.y - localAnchor.y : 0;
  const next: Record<string, { x: number; y: number }> = {};
  for (const node of nodes) {
    const position = local.positions[node.id];
    if (position) next[node.id] = { x: position.x + dx, y: position.y + dy };
  }
  return next;
}

interface OccupiedBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

function freeSpace(
  occupied: OccupiedBox[],
  gap: number,
  x: number,
  y: number,
  width: number,
  height: number,
): boolean {
  return !occupied.some((box) => x < box.x + box.width + gap
    && x + width + gap > box.x
    && y < box.y + box.height + gap
    && y + height + gap > box.y);
}

function findFreeSpot(
  originX: number,
  originY: number,
  width: number,
  height: number,
  stepX: number,
  stepY: number,
  isFree: (x: number, y: number) => boolean,
): { x: number; y: number } {
  for (let radius = 1; radius <= 24; radius += 1) {
    for (let row = -radius; row <= radius; row += 1) {
      for (let column = -radius; column <= radius; column += 1) {
        if (Math.max(Math.abs(column), Math.abs(row)) !== radius) continue;
        const x = originX + column * stepX;
        const y = originY + row * stepY;
        if (isFree(x, y)) return { x, y };
      }
    }
  }
  return { x: originX, y: originY };
}

function mergeNewPositions(
  current: Record<string, { x: number; y: number }>,
  next: Record<string, { x: number; y: number }>,
  nodes: Array<{ id: string; width: number; height: number }>,
  selectedId: string | null,
): Record<string, { x: number; y: number }> {
  // Deep navigation only adds a focus slice. Keep already-placed nodes
  // exactly where they are; place only the newly revealed nodes.
  const anchorId = selectedId && next[selectedId]
    ? selectedId
    : nodes.find((node) => current[node.id] && next[node.id])?.id;
  const anchor = anchorId ? current[anchorId] : null;
  const anchorNext = anchorId ? next[anchorId] : null;
  const dx = anchor && anchorNext ? anchor.x - anchorNext.x : 0;
  const dy = anchor && anchorNext ? anchor.y - anchorNext.y : 0;
  const sizes = new Map(nodes.map((node) => [node.id, { width: node.width, height: node.height }]));
  const gap = 18;
  const stepX = Math.max(160, ...nodes.map((node) => node.width)) + gap;
  const stepY = Math.max(64, ...nodes.map((node) => node.height)) + gap;
  const occupied: OccupiedBox[] = Object.entries(current).map(([id, position]) => ({
    x: position.x,
    y: position.y,
    width: sizes.get(id)?.width ?? 170,
    height: sizes.get(id)?.height ?? 68,
  }));
  const merged = { ...current };
  const newIds = Object.keys(next).filter((id) => !(id in current)).sort();
  for (const id of newIds) {
    const size = sizes.get(id) ?? { width: 170, height: 68 };
    const proposed = next[id];
    const originX = proposed.x + dx;
    const originY = proposed.y + dy;
    const spot = freeSpace(occupied, gap, originX, originY, size.width, size.height)
      ? { x: originX, y: originY }
      : findFreeSpot(
          originX,
          originY,
          size.width,
          size.height,
          stepX,
          stepY,
          (x, y) => freeSpace(occupied, gap, x, y, size.width, size.height),
        );
    merged[id] = spot;
    occupied.push({ ...spot, width: size.width, height: size.height });
  }
  return merged;
}

function frameNodesInView(
  flow: ReactFlowInstance,
  nodes: PositionedNode[],
): void {
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
  flow.setCenter(
    (box.minX + box.maxX) / 2,
    (box.minY + box.maxY) / 2,
    { zoom: Math.max(0.25, zoom), duration: 0 },
  );
}

async function exportGraphPng(nodes: FlowNode[]): Promise<void> {
  if (nodes.length === 0) return;
  const bounds = getNodesBounds(nodes);
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
}

export type GraphMode = 'focus' | 'path' | 'impact';

interface NodeClickDeps {
  mode: GraphMode;
  report: GraphReport | null;
  path: PathResult | null;
  showImpact: (node: GraphNode, direction: 'in' | 'out') => Promise<void>;
  setPathIndex: (index: number) => void;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  selectAndCenter: (id: string) => void;
  focusWithoutHistory: (id: string) => void;
  clickTimer: { current: number | null };
}

function createNodeClickHandlers({
  mode,
  report,
  path,
  showImpact,
  setPathIndex,
  setSelectedId,
  selectAndCenter,
  focusWithoutHistory,
  clickTimer,
}: NodeClickDeps): { onNodeClick: NodeMouseHandler; onNodeDoubleClick: NodeMouseHandler } {
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
    // Give a double-click a chance to cancel the single-click focus. Focusing
    // immediately repacks the grid, so the second click would land elsewhere.
    if (clickTimer.current !== null) {
      window.clearTimeout(clickTimer.current);
      clickTimer.current = null;
    }
    clickTimer.current = window.setTimeout(() => {
      clickTimer.current = null;
      selectAndCenter(node.id);
    }, 220);
  };
  const onNodeDoubleClick: NodeMouseHandler = (_event, node) => {
    if (clickTimer.current !== null) {
      window.clearTimeout(clickTimer.current);
      clickTimer.current = null;
    }
    focusWithoutHistory(node.id);
  };
  return { onNodeClick, onNodeDoubleClick };
}

interface EntryLoadingDeps {
  apiContext: { base: string; graphId: string } | null;
  snapshot: GraphSummary | null;
  focusRequest: { current: number };
  setReport: Dispatch<SetStateAction<GraphReport | null>>;
  setEntryRoots: Dispatch<SetStateAction<EntryPoint[]>>;
  setEntryInfo: Dispatch<SetStateAction<{
    totalRoots: number;
    truncated: boolean;
  } | null>>;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  setFocusIds: Dispatch<SetStateAction<Set<string> | null>>;
  clearHistory: () => void;
  setRouteIds: Dispatch<SetStateAction<Set<string>>>;
  setPath: Dispatch<SetStateAction<PathResult | null>>;
  setPathIndexState: Dispatch<SetStateAction<number>>;
  setFilters: Dispatch<SetStateAction<Filters>>;
  setSearchText: Dispatch<SetStateAction<string>>;
  setError: (message: string | null) => void;
}

async function loadEntryPoints({
  apiContext,
  snapshot,
  focusRequest,
  setReport,
  setEntryRoots,
  setEntryInfo,
  setSelectedId,
  setFocusIds,
  clearHistory,
  setRouteIds,
  setPath,
  setPathIndexState,
  setFilters,
  setSearchText,
  setError,
}: EntryLoadingDeps): Promise<void> {
  if (!apiContext || !snapshot) return;
  focusRequest.current += 1;
  try {
    // Fill the overview root by root: every entry point gets its first-level
    // calls; only when the node budget still has room are second-level nodes
    // added. The API slice already uses this level ordering.
    const data = await fetchEntryPoints(apiContext.base, apiContext.graphId, ENTRY_ROOTS, 2, ENTRY_LIMIT);
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
      truncated: data.truncated,
    });
    setSelectedId(null);
    setFocusIds(null);
    clearHistory();
    setRouteIds(new Set());
    setPath(null);
    setPathIndexState(0);
    setFilters(defaultFilters());
    setSearchText('');
    // Do not clear positions here: a second identical entry-point response
    // would leave an empty position map without changing the layout key.
  } catch (caught) {
    setError(`Entry points failed: ${(caught as Error).message}`);
  }
}

interface FocusSliceDeps {
  snapshot: GraphSummary | null;
  focusHistoryRef: { current: string[] };
  mergeSlice: (slice: GraphSlice, focus: Set<string> | null) => void;
  setEntryRoots: Dispatch<SetStateAction<EntryPoint[]>>;
  setEntryInfo: Dispatch<SetStateAction<{
    totalRoots: number;
    truncated: boolean;
  } | null>>;
  setPath: Dispatch<SetStateAction<PathResult | null>>;
  setPathIndexState: Dispatch<SetStateAction<number>>;
  setRouteIds: Dispatch<SetStateAction<Set<string>>>;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
}

function applyFocusedSlice(slice: FocusSlice, id: string, {
  snapshot,
  focusHistoryRef,
  mergeSlice,
  setEntryRoots,
  setEntryInfo,
  setPath,
  setPathIndexState,
  setRouteIds,
  setSelectedId,
}: FocusSliceDeps): void {
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
  for (const historyId of focusHistoryRef.current) focus.add(historyId);
  // Merge rather than replace: previously placed nodes keep their positions
  // when the user walks deeper through the call graph.
  mergeSlice(slice, focus);
  setEntryRoots([]);
  setEntryInfo(null);
  setPath(null);
  setPathIndexState(0);
  setRouteIds(new Set(slice.route));
  setSelectedId(id);
}

interface ResetDeps {
  focusRequest: { current: number };
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  setFocusIds: Dispatch<SetStateAction<Set<string> | null>>;
  clearHistory: () => void;
  setRouteIds: Dispatch<SetStateAction<Set<string>>>;
  setPath: Dispatch<SetStateAction<PathResult | null>>;
  setPathIndexState: Dispatch<SetStateAction<number>>;
  setEntryRoots: Dispatch<SetStateAction<EntryPoint[]>>;
  setEntryInfo: Dispatch<SetStateAction<{
    totalRoots: number;
    truncated: boolean;
  } | null>>;
  setFilters: Dispatch<SetStateAction<Filters>>;
  setSearchText: Dispatch<SetStateAction<string>>;
  resetPositions: () => void;
}

function resetViewState({
  focusRequest,
  setSelectedId,
  setFocusIds,
  clearHistory,
  setRouteIds,
  setPath,
  setPathIndexState,
  setEntryRoots,
  setEntryInfo,
  setFilters,
  setSearchText,
  resetPositions,
}: ResetDeps): void {
  focusRequest.current += 1;
  setSelectedId(null);
  setFocusIds(null);
  clearHistory();
  setRouteIds(new Set());
  setPath(null);
  setPathIndexState(0);
  setEntryRoots([]);
  setEntryInfo(null);
  setFilters(defaultFilters());
  setSearchText('');
  resetPositions();
}

interface Input {
  report: GraphReport | null;
  snapshot: GraphSummary | null;
  allNodes: GraphNode[];
  allEdges: GraphEdge[];
  setReport: Dispatch<SetStateAction<GraphReport | null>>;
  apiContext: { base: string; graphId: string } | null;
  setError: (message: string | null) => void;
  mode: GraphMode;
}

const FOCUS_DEPTH = 1;
const FOCUS_LIMIT = 180;
const ENTRY_ROOTS = 50;
const ENTRY_LIMIT = 180;

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
  entryRoots: EntryPoint[];
  entryInfo: { totalRoots: number; truncated: boolean } | null;
  openEntryPoints: () => Promise<void>;
  mergeSlice: (slice: GraphSlice, focus: Set<string> | null) => void;
  focusNode: (id: string) => void;
  focusWithoutHistory: (id: string) => void;
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

function canonicalLayeredPositions(
  nodes: GraphNode[],
  edges: GraphEdge[],
): Record<string, { x: number; y: number }> {
  if (nodes.length === 0) return {};
  const { columnWidth, rowHeight } = layoutSpacing(nodes);
  const layers = new Map<number, GraphNode[]>();
  for (const node of nodes) {
    const depth = Number.isFinite(node.metrics.depth)
      ? Math.max(0, Math.round(node.metrics.depth))
      : 0;
    const bucket = layers.get(depth) ?? [];
    bucket.push(node);
    layers.set(depth, bucket);
  }
  const incoming = incomingEdges(edges);
  const result: Record<string, { x: number; y: number }> = {};
  const rowById = new Map<string, number>();
  const orderedDepths = [...layers.keys()].sort((left, right) => left - right);
  for (const depth of orderedDepths) {
    const layer = layers.get(depth) ?? [];
    sortLayer(layer, incoming, rowById);
    layer.forEach((node, row) => {
      result[node.id] = { x: depth * columnWidth, y: row * rowHeight };
      rowById.set(node.id, row);
    });
  }
  return result;
}

interface LayoutInput {
  visible: { nodes: GraphNode[]; edges: GraphEdge[] };
  layoutSource: GraphNode[];
  allEdges: GraphEdge[];
  layoutScope: string;
  selectedId: string | null;
  entryRoots: EntryPoint[];
  focusIds: Set<string> | null;
  mode: GraphMode;
  path: PathResult | null;
  setError: (message: string | null) => void;
}

interface LayoutState {
  positions: Record<string, { x: number; y: number }>;
  layoutKind: LayoutKind;
  setLayoutKind: Dispatch<SetStateAction<LayoutKind>>;
  layoutNonce: number;
  relayout: () => void;
  layouting: boolean;
  resetPositions: () => void;
}

function useGraphLayout({
  visible,
  layoutSource,
  allEdges,
  layoutScope,
  selectedId,
  entryRoots,
  focusIds,
  mode,
  path,
  setError,
}: LayoutInput): LayoutState {
  const reactFlow = useReactFlow();
  const [positions, setPositions] = useState<Record<string, { x: number; y: number }>>({});
  const [layoutKind, setLayoutKind] = useState<LayoutKind>('layered');
  const [layouting, setLayouting] = useState(false);
  const [layoutNonce, setLayoutNonce] = useState(0);
  const lastLayoutConfig = useRef({ kind: '', nonce: -1, scope: '' });
  const selectedIdRef = useRef<string | null>(null);
  selectedIdRef.current = selectedId;
  const centeredPositions = useRef<Record<string, { x: number; y: number }> | null>(null);

  const canonicalPositions = useMemo(
    () => canonicalLayeredPositions(layoutSource, allEdges),
    [layoutSource, allEdges],
  );
  const positionedNodes = useMemo<PositionedNode[]>(
    () => visible.nodes.map((node) => {
      const size = nodeSize(node);
      return {
        id: node.id,
        position: positions[node.id] ?? { x: 0, y: 0 },
        width: size.width,
        height: size.height,
      };
    }),
    [visible.nodes, positions],
  );
  const layoutNodes = useMemo(
    () => visible.nodes.map((node) => ({ id: node.id, ...nodeSize(node) })),
    [visible.nodes],
  );
  const viewKey = useMemo(
    () =>
      `${layoutScope}|${layoutKind}|${layoutNonce}|${selectedId ?? ''}|${layoutNodes.map((node) => node.id).join('|')}|${visible.edges
        .map((edge) => edge.id)
        .join('|')}`,
    [layoutScope, layoutKind, layoutNonce, layoutNodes, selectedId, visible.edges],
  );

  useEffect(() => {
    if (layoutNodes.length === 0) {
      setPositions({});
      return;
    }
    if (layoutKind === 'layered') {
      const selected = selectedIdRef.current;
      const roots = selected ? [selected] : entryRoots.map((root) => root.id);
      const viewport = reactFlow.getViewport();
      const host = document.querySelector('.react-flow')?.getBoundingClientRect();
      const center = host
        ? {
            x: (host.width / 2 - viewport.x) / viewport.zoom,
            y: (host.height / 2 - viewport.y) / viewport.zoom,
          }
        : { x: 0, y: 0 };
      setPositions(
        layeredPositionsForView(visible.nodes, visible.edges, selected, roots, focusIds, center),
      );
      setLayouting(false);
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
        setPositions((current) => (
          preserve ? mergeNewPositions(current, next, layoutNodes, selectedIdRef.current) : next
        ));
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
  }, [viewKey, layoutScope, canonicalPositions, entryRoots, focusIds]);

  const frameNodes = useCallback(
    (nodes: PositionedNode[]) => frameNodesInView(reactFlow, nodes),
    [reactFlow],
  );

  useEffect(() => {
    if (Object.keys(positions).length === 0 || centeredPositions.current === positions) return;
    centeredPositions.current = positions;
    // Path mode fits the whole route; centering the endpoint would push the
    // entry and middle steps off-screen.
    if ((mode === 'path' && path) || focusIds !== null) return;

    // Entry points are an overview: fit the whole visible slice once instead
    // of centering the first root, which leaves most of it off-screen.
    if (selectedId === null && entryRoots.length > 0) {
      const timer = window.setTimeout(() => frameNodes(positionedNodes), 120);
      return () => window.clearTimeout(timer);
    }

    const selected = selectedId ? positionedNodes.find((node) => node.id === selectedId) : null;
    const rootId = entryRoots[0]?.id;
    const root = rootId ? positionedNodes.find((node) => node.id === rootId) : null;
    const target = selected ?? root;
    const center = target
      ? {
          x: target.position.x + Number(target.width ?? 0) / 2,
          y: target.position.y + Number(target.height ?? 0) / 2,
        }
      : denseCenter(positionedNodes);
    if (!center) return;
    void reactFlow.setCenter(center.x, center.y, { zoom: reactFlow.getZoom(), duration: 0 });
  }, [positions, positionedNodes, entryRoots, selectedId, reactFlow, mode, path, focusIds, frameNodes]);

  // Path mode is about the whole route, not the endpoint under the cursor.
  useEffect(() => {
    if (mode !== 'path' || !path) return;
    const route = new Set(path.path);
    const routeNodes = positionedNodes.filter((node) => route.has(node.id));
    if (routeNodes.length < 2) return;
    const timer = window.setTimeout(() => frameNodes(routeNodes), 120);
    return () => window.clearTimeout(timer);
  }, [mode, path, positionedNodes, frameNodes]);

  // Navigating from a zoomed-out entry overview leaves a small focus slice at
  // the old overview zoom (cards become 83x34px). Keep the normal "focus does
  // not move the viewport" rule, but fit small focus slices when the current
  // zoom would make their labels unreadable.
  useEffect(() => {
    if (mode !== 'focus' || focusIds === null || selectedId === null) return;
    if (positionedNodes.length === 0 || positionedNodes.length > 40) return;
    if (reactFlow.getZoom() >= MIN_READABLE_ZOOM) return;
    const timer = window.setTimeout(() => frameNodes(positionedNodes), 120);
    return () => window.clearTimeout(timer);
  }, [mode, focusIds, selectedId, positionedNodes, reactFlow, frameNodes]);

  const relayout = useCallback(() => setLayoutNonce((value) => value + 1), []);
  const resetPositions = useCallback(() => setPositions({}), []);

  return {
    positions,
    layoutKind,
    setLayoutKind,
    layoutNonce,
    relayout,
    layouting,
    resetPositions,
  };
}

interface PathInput {
  path: PathResult | null;
  pathIndex: number;
  apiContext: { base: string; graphId: string } | null;
  mergeSlice: (slice: GraphSlice, focus: Set<string> | null) => void;
  setError: (message: string | null) => void;
  setPath: Dispatch<SetStateAction<PathResult | null>>;
  setPathIndexState: Dispatch<SetStateAction<number>>;
  setRouteIds: Dispatch<SetStateAction<Set<string>>>;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  setSearchText: Dispatch<SetStateAction<string>>;
  setFilters: Dispatch<SetStateAction<Filters>>;
  setFocusIds: Dispatch<SetStateAction<Set<string> | null>>;
  setEntryRoots: Dispatch<SetStateAction<EntryPoint[]>>;
  setEntryInfo: Dispatch<SetStateAction<{
    totalRoots: number;
    truncated: boolean;
  } | null>>;
}

interface PathActions {
  applyPath: (result: PathResult) => void;
  findPath: (fromId: string, toId: string) => Promise<boolean>;
  setPathIndex: (index: number) => void;
  showPathScope: () => void;
  clearRouteHighlight: () => void;
  clearPath: () => void;
}

function useGraphPath({
  path,
  pathIndex,
  apiContext,
  mergeSlice,
  setError,
  setPath,
  setPathIndexState,
  setRouteIds,
  setSelectedId,
  setSearchText,
  setFilters,
  setFocusIds,
  setEntryRoots,
  setEntryInfo,
}: PathInput): PathActions {
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

  return { applyPath, findPath, setPathIndex, showPathScope, clearRouteHighlight, clearPath };
}

export function useGraphView({
  report,
  snapshot,
  allNodes,
  allEdges,
  setReport,
  apiContext,
  setError,
  mode,
}: Input): GraphView {
  const reactFlow = useReactFlow();
  const [filters, setFilters] = useState<Filters>(() => defaultFilters());
  const [searchText, setSearchText] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [focusIds, setFocusIds] = useState<Set<string> | null>(null);
  const [focusHistory, setFocusHistory] = useState<string[]>([]);
  const focusHistoryRef = useRef<string[]>([]);
  const [routeIds, setRouteIds] = useState<Set<string>>(new Set());
  const [path, setPath] = useState<PathResult | null>(null);
  const [pathIndex, setPathIndexState] = useState(0);
  const [entryRoots, setEntryRoots] = useState<EntryPoint[]>([]);
  const [entryInfo, setEntryInfo] = useState<{
    totalRoots: number;
    truncated: boolean;
  } | null>(null);
  const clickTimer = useRef<number | null>(null);
  const focusRequest = useRef(0);

  const entryIds = useMemo(
    () => (entryRoots.length ? new Set(entryRoots.map((root) => root.id)) : null),
    [entryRoots],
  );

  const visible = useMemo(
    () => (report ? visibleGraph(report, filters, focusIds) : { nodes: [], edges: [] }),
    [report, filters, focusIds],
  );

  const layoutSource = allNodes.length > 0 ? allNodes : report?.nodes ?? [];
  const {
    positions,
    layoutKind,
    setLayoutKind,
    layoutNonce,
    relayout,
    layouting,
    resetPositions,
  } = useGraphLayout({
    visible,
    layoutSource,
    allEdges,
    layoutScope: report?.meta.generatedAt ?? '',
    selectedId,
    entryRoots,
    focusIds,
    mode,
    path,
    setError,
  });

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

  const historyEdgeIds = useMemo(() => {
    const result = new Set<string>();
    const edges = report?.edges ?? allEdges;
    for (let index = 1; index < focusHistory.length; index += 1) {
      const previous = focusHistory[index - 1];
      const current = focusHistory[index];
      for (const edge of edges) {
        if ((edge.source === previous && edge.target === current)
          || (edge.source === current && edge.target === previous)) {
          result.add(edge.id);
        }
      }
    }
    return result;
  }, [allEdges, focusHistory, report]);

  const hidden = useMemo(
    () => hiddenRelations(visible.nodes, visible.edges),
    [visible],
  );

  const flowNodes = useMemo<FlowNode[]>(
    () => toFlowNodes(visible.nodes, selectedId, neighborIds, entryIds, routeIds, hidden).map((node) => ({
      ...node,
      position: positions[node.id] ?? node.position,
    })),
    [visible.nodes, selectedId, neighborIds, entryIds, routeIds, hidden, positions],
  );

  const obstacles = useMemo(
    () => visible.nodes.flatMap((node) => {
      const position = positions[node.id];
      if (!position) return [];
      const size = nodeSize(node);
      return [{ id: node.id, x: position.x, y: position.y, width: size.width, height: size.height }];
    }),
    [visible.nodes, positions],
  );

  const flowEdges = useMemo<FlowEdge[]>(
    () => toFlowEdges(visible.edges, selectedId, neighborIds, routeIds, historyEdgeIds, obstacles, mode),
    [visible.edges, selectedId, neighborIds, routeIds, historyEdgeIds, obstacles, mode],
  );

  const rememberFocus = useCallback((id: string) => {
    const current = focusHistoryRef.current;
    const existing = current.lastIndexOf(id);
    // Returning to an earlier node rewinds the focus history: everything after
    // that node is a forward branch the user just discarded.
    const next = existing >= 0 ? current.slice(0, existing + 1) : [...current, id];
    if (next.length === current.length && next.every((value, index) => value === current[index])) {
      return;
    }
    focusHistoryRef.current = next;
    setFocusHistory(next);
  }, []);

  const clearHistory = useCallback(() => {
    focusHistoryRef.current = [];
    setFocusHistory([]);
  }, []);

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
        focusRequest.current += 1;
        setSelectedId(null);
        setFocusIds(null);
        clearHistory();
      }
    }, 180);
    return () => window.clearTimeout(timer);
  }, [clearHistory, searchText]);

  const showFocusedSlice = useCallback((slice: FocusSlice, id: string) => {
    applyFocusedSlice(slice, id, {
      snapshot,
      focusHistoryRef,
      mergeSlice,
      setEntryRoots,
      setEntryInfo,
      setPath,
      setPathIndexState,
      setRouteIds,
      setSelectedId,
    });
  }, [mergeSlice, snapshot, setEntryInfo, setEntryRoots, setSelectedId]);

  const selectAndCenter = useCallback((id: string) => {
    if (mode === 'focus') rememberFocus(id);
    // A node selection is a new exploration scope: reveal only its connected
    // slice so the node budget is spent on neighbours, not on unrelated entry
    // points that happen to be in the previous view.
    setSearchText('');
    setFilters((current) => (current.search ? { ...current, search: '' } : current));

    if (!apiContext || !snapshot) {
      // Local JSON report: filter the in-memory graph around the selection.
      if (!report) return;
      focusRequest.current += 1;
      setSelectedId(id);
      setEntryRoots([]);
      setEntryInfo(null);
      setRouteIds(new Set());
      const focus = neighborhood(report, id, FOCUS_DEPTH);
      for (const historyId of focusHistoryRef.current) focus.add(historyId);
      setFocusIds(focus);
      return;
    }

    // Keep the previous focus and its edges until the new slice arrives. Setting
    // selectedId first would scope the old edges to a node that is not in the
    // old focus, making every link disappear during the request.
    const request = focusRequest.current + 1;
    focusRequest.current = request;
    // Direct neighbours, the shortest entry→selection→exit route, and the
    // surrounding two-hop context, with the same node budget as the initial view.
    void fetchFocus(apiContext.base, apiContext.graphId, id, FOCUS_LIMIT)
      .then((slice) => {
        if (request === focusRequest.current) showFocusedSlice(slice, id);
      })
      .catch((caught: Error) => {
        if (request === focusRequest.current) setError(caught.message);
      });
  }, [
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
    mode,
    rememberFocus,
  ]);

  const focusNode = useCallback((id: string) => {
    selectAndCenter(id);
  }, [selectAndCenter]);

  const focusWithoutHistory = useCallback((id: string) => {
    clearHistory();
    selectAndCenter(id);
  }, [clearHistory, selectAndCenter]);

  const clearFocus = useCallback(() => {
    focusRequest.current += 1;
    setFocusIds(null);
    setSelectedId(null);
    clearHistory();
  }, [clearHistory]);

  const clearSelection = useCallback(() => {
    setSelectedId(null);
    reactFlow.setNodes((nodes) => nodes.map((node) => ({ ...node, selected: false })));
  }, [reactFlow]);

  const {
    applyPath,
    findPath,
    setPathIndex,
    showPathScope,
    clearRouteHighlight,
    clearPath,
  } = useGraphPath({
    path,
    pathIndex,
    apiContext,
    mergeSlice,
    setError,
    setPath,
    setPathIndexState,
    setRouteIds,
    setSelectedId,
    setSearchText,
    setFilters,
    setFocusIds,
    setEntryRoots,
    setEntryInfo,
  });

  const openNeighborhood = useCallback(async (node: GraphNode) => {
    selectAndCenter(node.id);
  }, [selectAndCenter]);

  const openEntryPoints = useCallback(() => loadEntryPoints({
    apiContext,
    snapshot,
    focusRequest,
    setReport,
    setEntryRoots,
    setEntryInfo,
    setSelectedId,
    setFocusIds,
    clearHistory,
    setRouteIds,
    setPath,
    setPathIndexState,
    setFilters,
    setSearchText,
    setError,
  }), [apiContext, clearHistory, snapshot, setError, setReport]);

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

  const exportPng = useCallback(() => exportGraphPng(flowNodes), [flowNodes]);

  const { onNodeClick, onNodeDoubleClick } = createNodeClickHandlers({
    mode,
    report,
    path,
    showImpact,
    setPathIndex,
    setSelectedId,
    selectAndCenter,
    focusWithoutHistory,
    clickTimer,
  });

  const resetGraphView = useCallback(() => resetViewState({
    focusRequest,
    setSelectedId,
    setFocusIds,
    clearHistory,
    setRouteIds,
    setPath,
    setPathIndexState,
    setEntryRoots,
    setEntryInfo,
    setFilters,
    setSearchText,
    resetPositions,
  }), [clearHistory, resetPositions]);

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
    relayout,
    layouting,
    focusIds,
    entryRoots,
    entryInfo,
    openEntryPoints,
    mergeSlice,
    focusNode,
    focusWithoutHistory,
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
