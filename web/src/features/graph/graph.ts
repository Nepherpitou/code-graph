import type { Edge, Node } from '@xyflow/react';
import type { GraphEdge, GraphNode, GraphReport } from '../../types';

export const CARD_WIDTH = 212;
export const CARD_HEIGHT = 86;
export const CONSTANT_WIDTH = 176;
export const CONSTANT_HEIGHT = 58;

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
}

export interface FlowEdgeData extends Record<string, unknown> {
  edge: GraphEdge;
  dimmed: boolean;
  highlighted: boolean;
  route?: boolean;
}

export interface GhostNodeData extends Record<string, unknown> {
  ghost: true;
  hidden: number;
  hiddenIn: number;
  hiddenOut: number;
}

export interface GhostEdgeData extends Record<string, unknown> {
  ghost: true;
  hidden: number;
  nodeWidth: number;
  nodeHeight: number;
}

export type FlowNode = Node<FlowNodeData, 'card'>;
export type GhostFlowNode = Node<GhostNodeData, 'ghost'>;
export type AnyFlowNode = FlowNode | GhostFlowNode;
export type FlowEdge = Edge<FlowEdgeData | GhostEdgeData>;

/** Length of the fading stub that points at connections outside the slice. */
export const GHOST_STUB = 42;

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

export function toGhostNodes(
  nodes: GraphNode[],
  hidden: Map<string, HiddenRelations>,
  positions: Record<string, { x: number; y: number }>,
): GhostFlowNode[] {
  return nodes.flatMap((node) => {
    const relation = hidden.get(node.id);
    const position = positions[node.id];
    if (!relation || !position) return [];
    const size = nodeSize(node);
    return [{
      id: `ghost:${node.id}`,
      type: 'ghost',
      position: {
        x: position.x + size.width + GHOST_STUB,
        y: position.y + size.height / 2,
      },
      width: 1,
      height: 1,
      style: { width: 1, height: 1, opacity: 0, pointerEvents: 'none' },
      selectable: false,
      draggable: false,
      focusable: false,
      data: {
        ghost: true,
        hidden: relation.hidden,
        hiddenIn: relation.hiddenIn,
        hiddenOut: relation.hiddenOut,
      },
    }];
  });
}

export function toGhostEdges(
  nodes: GraphNode[],
  hidden: Map<string, HiddenRelations>,
  positions: Record<string, { x: number; y: number }>,
): FlowEdge[] {
  return nodes.flatMap((node) => {
    const relation = hidden.get(node.id);
    if (!relation || !positions[node.id]) return [];
    const size = nodeSize(node);
    return [{
      id: `ghost-edge:${node.id}`,
      source: node.id,
      target: `ghost:${node.id}`,
      type: 'ghost',
      selectable: false,
      focusable: false,
      interactionWidth: 0,
      data: {
        ghost: true,
        hidden: relation.hidden,
        nodeWidth: size.width,
        nodeHeight: size.height,
      },
    }];
  });
}

export function toFlowEdges(
  edges: GraphEdge[],
  selectedId: string | null,
  neighborIds: Set<string>,
  routeIds: Set<string> = new Set(),
): FlowEdge[] {
  return edges.map((edge) => {
    const inferred = Boolean(edge.inferred);
    const data = edge.kind === 'data';
    const returnsValue = edge.kind === 'call' && (edge.resultUsed === 'all' || edge.resultUsed === 'some');
    const unknown = edge.kind === 'call' && edge.resultUsed === 'unknown';
    const route = routeIds.has(edge.source) && routeIds.has(edge.target);
    const color = route
      ? 'var(--success)'
      : inferred ? 'var(--warning)' : data ? 'var(--success)' : returnsValue ? 'var(--accent)' : 'var(--edge-call)';
    const dash = route ? undefined : inferred || data || unknown ? '6 4' : undefined;
    const highlighted = selectedId !== null && (edge.source === selectedId || edge.target === selectedId);
    const dimmed = selectedId !== null && !highlighted && !(neighborIds.has(edge.source) && neighborIds.has(edge.target));
    return {
      id: edge.id,
      source: edge.source,
      target: edge.target,
      type: 'smoothstep',
      animated: false,
      label: edge.kind === 'call' && edge.sites > 1 ? `${edge.sites}×` : undefined,
      labelStyle: { fill: 'var(--muted)', fontSize: 10, fontFamily: MONO },
      labelBgStyle: { fill: 'var(--panel)', fillOpacity: 0.9 },
      labelBgPadding: [4, 2],
      labelBgBorderRadius: 4,
      interactionWidth: 16,
      data: { edge, dimmed, highlighted, route },
      style: {
        stroke: route ? 'var(--route)' : highlighted ? 'var(--accent)' : color,
        strokeWidth: route ? 4 : highlighted ? 2.6 : 1.5,
        strokeDasharray: dash,
        opacity: dimmed ? 0.1 : route ? 1 : 0.9,
      },
      markerEnd: {
        type: 'arrowclosed',
        color: route ? 'var(--route)' : highlighted ? 'var(--accent)' : color,
        width: route ? 21 : 16,
        height: route ? 21 : 16,
      },
      markerStart: returnsValue
        ? {
            type: 'arrowclosed',
            color: route ? 'var(--route)' : highlighted ? 'var(--accent)' : color,
            width: 14,
            height: 14,
          }
        : undefined,
    } satisfies FlowEdge;
  });
}
