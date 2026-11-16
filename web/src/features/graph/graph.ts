import type { Edge, Node } from '@xyflow/react';
import type { GraphEdge, GraphNode, GraphReport } from '../../types';

export const CARD_WIDTH = 212;
export const CARD_HEIGHT = 86;
export const CONSTANT_WIDTH = 176;
// Variables/constants still have two title lines plus the metrics row. At the
// old 58px the content overflowed and the top/bottom padding disappeared.
export const CONSTANT_HEIGHT = 76;

/** Below this zoom the card's 12px name is under 9px, so text is no longer readable. */
export const MIN_READABLE_ZOOM = 0.8;
/** React Flow's culling has overhead, but pays off once the canvas holds this many cards. */
export const CULL_THRESHOLD = 150;

export const KIND_COLORS: Record<string, string> = {
  function: '#4f8ef7',
  method: '#8b5cf6',
  constructor: '#e6a23c',
  constant: '#2fb344',
  variable: '#14b8a6',
  other: '#9ca3af',
};

export interface Filters {
  kinds: Set<string>;
  minFanIn: number;
  minFanOut: number;
  hideDead: boolean;
  hideExternal: boolean;
  cyclesOnly: boolean;
  hideIsolated: boolean;
  search: string;
}

export interface FlowNodeData extends Record<string, unknown> {
  node: GraphNode;
  selected: boolean;
  dimmed: boolean;
  highlighted: boolean;
  entryPoint: boolean;
  route: boolean;
  hidden: HiddenRelations | null;
}

export interface EdgeObstacle {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface FlowEdgeData extends Record<string, unknown> {
  edge: GraphEdge;
  dimmed: boolean;
  highlighted: boolean;
  route?: boolean;
  history?: boolean;
  obstacles?: EdgeObstacle[];
  lane?: number;
}

export type FlowNode = Node<FlowNodeData, 'card'>;
export type FlowEdge = Edge<FlowEdgeData>;

export function defaultFilters(): Filters {
  return {
    kinds: new Set(['function', 'method', 'constructor', 'constant', 'variable']),
    minFanIn: 0,
    minFanOut: 0,
    hideDead: false,
    hideExternal: true,
    cyclesOnly: false,
    hideIsolated: false,
    search: '',
  };
}

export function isVisible(node: GraphNode, filters: Filters, focusIds: Set<string> | null): boolean {
  if (node.external && filters.hideExternal) return false;
  if (!filters.kinds.has(node.kind)) return false;
  if (node.metrics.fanIn < filters.minFanIn) return false;
  if (node.metrics.fanOut < filters.minFanOut) return false;
  if (filters.hideDead && node.metrics.deadCodeCandidate) return false;
  if (filters.cyclesOnly && !node.metrics.inCycle) return false;
  if (filters.hideIsolated && node.metrics.fanIn + node.metrics.fanOut === 0) return false;
  if (focusIds && !focusIds.has(node.id)) return false;
  if (filters.search) {
    const haystack = `${node.qname} ${node.file} ${node.kind}`.toLowerCase();
    if (!haystack.includes(filters.search.toLowerCase())) return false;
  }
  return true;
}

function byId(left: { id: string }, right: { id: string }): number {
  return left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
}

export function visibleGraph(report: GraphReport, filters: Filters, focusIds: Set<string> | null): { nodes: GraphNode[]; edges: GraphEdge[] } {
  // API maps have no render order. Keep the visible graph canonical so the same
  // selection always feeds ELK the same input and produces the same layout.
  const nodes = report.nodes.filter((node) => isVisible(node, filters, focusIds)).sort(byId);
  const ids = new Set(nodes.map((node) => node.id));
  const edges = report.edges
    .filter((edge) => ids.has(edge.source) && ids.has(edge.target))
    .sort((left, right) => left.source.localeCompare(right.source)
      || left.target.localeCompare(right.target)
      || byId(left, right));
  return { nodes, edges };
}

export function neighborhood(report: GraphReport, startId: string, depth = 2): Set<string> {
  const seen = new Set<string>([startId]);
  let frontier = [startId];
  for (let step = 0; step < depth; step += 1) {
    const next: string[] = [];
    for (const id of frontier) {
      for (const edge of report.edges) {
        const other = edge.source === id ? edge.target : edge.target === id ? edge.source : null;
        if (other && !seen.has(other)) {
          seen.add(other);
          next.push(other);
        }
      }
    }
    frontier = next;
    if (!frontier.length) break;
  }
  return seen;
}

export function nodeSize(node: GraphNode): { width: number; height: number } {
  if (node.kind === 'constant' || node.kind === 'variable') {
    return { width: CONSTANT_WIDTH, height: CONSTANT_HEIGHT };
  }
  return { width: CARD_WIDTH, height: CARD_HEIGHT };
}

export function toFlowNodes(
  nodes: GraphNode[],
  selectedId: string | null,
  neighborIds: Set<string>,
  entryIds: Set<string> | null,
  routeIds: Set<string> = new Set(),
  hidden: Map<string, HiddenRelations> = new Map(),
): FlowNode[] {
  return nodes.map((node) => {
    const size = nodeSize(node);
    return {
      id: node.id,
      type: 'card',
      position: { x: 0, y: 0 },
      width: size.width,
      height: size.height,
      style: { width: `${size.width}px`, height: `${size.height}px` },
      data: {
        node,
        selected: node.id === selectedId,
        dimmed: selectedId !== null && node.id !== selectedId && !neighborIds.has(node.id) && !routeIds.has(node.id),
        highlighted: selectedId !== null && (node.id === selectedId || neighborIds.has(node.id) || routeIds.has(node.id)),
        entryPoint: entryIds?.has(node.id) ?? false,
        route: routeIds.has(node.id),
        hidden: hidden.get(node.id) ?? null,
      },
    };
  });
}

const MONO = 'ui-monospace, SFMono-Regular, Menlo, monospace';

export interface HiddenRelations {
  hiddenIn: number;
  hiddenOut: number;
  hidden: number;
}

/**
 * Compare full node metrics with the edges present in the current slice.
 * Fan-in/fan-out are computed over the whole graph, so the difference is the
 * number of distinct neighbours that exist but are not drawn here.
 */
export function hiddenRelations(
  nodes: GraphNode[],
  edges: GraphEdge[],
): Map<string, HiddenRelations> {
  const incoming = new Map<string, Set<string>>();
  const outgoing = new Map<string, Set<string>>();
  for (const edge of edges) {
    const sources = incoming.get(edge.target) ?? new Set<string>();
    sources.add(edge.source);
    incoming.set(edge.target, sources);
    const targets = outgoing.get(edge.source) ?? new Set<string>();
    targets.add(edge.target);
    outgoing.set(edge.source, targets);
  }
  const hidden = new Map<string, HiddenRelations>();
  for (const node of nodes) {
    const hiddenIn = Math.max(0, node.metrics.fanIn - (incoming.get(node.id)?.size ?? 0));
    const hiddenOut = Math.max(0, node.metrics.fanOut - (outgoing.get(node.id)?.size ?? 0));
    if (hiddenIn > 0 || hiddenOut > 0) {
      hidden.set(node.id, { hiddenIn, hiddenOut, hidden: hiddenIn + hiddenOut });
    }
  }
  return hidden;
}

export function toFlowEdges(
  edges: GraphEdge[],
  selectedId: string | null,
  neighborIds: Set<string>,
  routeIds: Set<string> = new Set(),
  historyEdgeIds: Set<string> = new Set(),
  obstacles: EdgeObstacle[] = [],
  mode: 'focus' | 'path' | 'impact' = 'focus',
): FlowEdge[] {
  const scoped = edges.filter((edge) => {
    if (selectedId === null) return true;
    if (mode === 'path' && routeIds.size > 0) return routeIds.has(edge.source) && routeIds.has(edge.target);
    if (mode === 'impact') return edge.target === selectedId;
    if (mode === 'focus') {
      return edge.source === selectedId
        || edge.target === selectedId
        || (routeIds.has(edge.source) && routeIds.has(edge.target))
        || historyEdgeIds.has(edge.id);
    }
    return true;
  });
  const outgoing = new Map<string, GraphEdge[]>();
  for (const edge of scoped) {
    const list = outgoing.get(edge.source) ?? [];
    list.push(edge);
    outgoing.set(edge.source, list);
  }
  const laneById = new Map<string, number>();
  for (const list of outgoing.values()) {
    const ordered = [...list].sort((left, right) => {
      const leftTarget = obstacles.find((box) => box.id === left.target);
      const rightTarget = obstacles.find((box) => box.id === right.target);
      return (leftTarget?.y ?? 0) - (rightTarget?.y ?? 0)
        || left.target.localeCompare(right.target)
        || left.id.localeCompare(right.id);
    });
    ordered.forEach((edge, index) => {
      laneById.set(edge.id, index - (ordered.length - 1) / 2);
    });
  }
  return scoped.map((edge) => {
    const inferred = Boolean(edge.inferred);
    const data = edge.kind === 'data';
    const returnsValue = edge.kind === 'call' && (edge.resultUsed === 'all' || edge.resultUsed === 'some');
    const unknown = edge.kind === 'call' && edge.resultUsed === 'unknown';
    const route = routeIds.has(edge.source) && routeIds.has(edge.target);
    const history = mode === 'focus' && historyEdgeIds.has(edge.id);
    const color = history
      ? 'var(--danger)'
      : route ? 'var(--success)'
        : inferred ? 'var(--warning)' : data ? 'var(--success)' : returnsValue ? 'var(--accent)' : 'var(--edge-call)';
    const dash = route || history ? undefined : inferred || data || unknown ? '6 4' : undefined;
    const highlighted = selectedId !== null && (edge.source === selectedId || edge.target === selectedId || history);
    const dimmed = selectedId !== null
      && !highlighted
      && !route
      && !history
      && !(neighborIds.has(edge.source) && neighborIds.has(edge.target));
    // A call graph is a context view, not a hairball: only the selected node's
    // relationships and the execution route deserve visual weight.
    const showLabel = edge.kind === 'call' && edge.sites > 1 && (highlighted || route || history);
    const sourceBox = obstacles.find((box) => box.id === edge.source);
    const targetBox = obstacles.find((box) => box.id === edge.target);
    const sameColumn = Boolean(sourceBox && targetBox && Math.abs(sourceBox.x - targetBox.x) < 1);
    const upward = sameColumn && Boolean(sourceBox && targetBox && targetBox.y < sourceBox.y);
    const forward = !targetBox || !sourceBox || targetBox.x >= sourceBox.x;
    const sourceHandle = sameColumn
      ? upward ? 'source-top' : 'source-bottom'
      : forward ? 'source-right' : 'source-left';
    const targetHandle = sameColumn
      ? upward ? 'target-bottom' : 'target-top'
      : forward ? 'target-left' : 'target-right';
    return {
      id: edge.id,
      source: edge.source,
      target: edge.target,
      sourceHandle,
      targetHandle,
      type: 'lane',
      animated: false,
      label: showLabel ? `${edge.sites}×` : undefined,
      labelStyle: { fill: 'var(--muted)', fontSize: 10, fontFamily: MONO },
      labelBgStyle: { fill: 'var(--panel)', fillOpacity: 0.9 },
      labelBgPadding: [4, 2],
      labelBgBorderRadius: 4,
      interactionWidth: 18,
      data: { edge, dimmed, highlighted, route, history, obstacles, lane: laneById.get(edge.id) ?? 0 },
      style: {
        stroke: history ? 'var(--danger)' : route ? 'var(--route)' : highlighted ? 'var(--accent)' : color,
        strokeWidth: history ? 3.2 : route ? 4 : highlighted ? 2.4 : 1.2,
        strokeDasharray: dash,
        opacity: dimmed ? 0.08 : history || route || highlighted ? 1 : 0.34,
      },
      markerEnd: {
        type: 'arrowclosed',
        color: history ? 'var(--danger)' : route ? 'var(--route)' : highlighted ? 'var(--accent)' : color,
        width: 20,
        height: 20,
        markerUnits: 'userSpaceOnUse',
      },
      markerStart: returnsValue
        ? {
            type: 'arrowclosed',
            color: history ? 'var(--danger)' : route ? 'var(--route)' : highlighted ? 'var(--accent)' : color,
            width: 20,
            height: 20,
            markerUnits: 'userSpaceOnUse',
          }
        : undefined,
    } satisfies FlowEdge;
  });
}
