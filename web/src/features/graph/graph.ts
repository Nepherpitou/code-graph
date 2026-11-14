import type { Edge, Node } from '@xyflow/react';
import type { GraphEdge, GraphNode, GraphReport } from '../../types';

export const CARD_WIDTH = 212;
export const CARD_HEIGHT = 86;
export const CONSTANT_WIDTH = 176;
export const CONSTANT_HEIGHT = 58;

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
  dimmed: boolean;
  highlighted: boolean;
}

export interface FlowEdgeData extends Record<string, unknown> {
  edge: GraphEdge;
  dimmed: boolean;
  highlighted: boolean;
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

export function visibleGraph(report: GraphReport, filters: Filters, focusIds: Set<string> | null): { nodes: GraphNode[]; edges: GraphEdge[] } {
  const nodes = report.nodes.filter((node) => isVisible(node, filters, focusIds));
  const ids = new Set(nodes.map((node) => node.id));
  const edges = report.edges.filter((edge) => ids.has(edge.source) && ids.has(edge.target));
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
        dimmed: selectedId !== null && node.id !== selectedId && !neighborIds.has(node.id),
        highlighted: selectedId !== null && (node.id === selectedId || neighborIds.has(node.id)),
      },
    };
  });
}

const MONO = 'ui-monospace, SFMono-Regular, Menlo, monospace';

export function toFlowEdges(
  edges: GraphEdge[],
  selectedId: string | null,
  neighborIds: Set<string>,
): FlowEdge[] {
  return edges.map((edge) => {
    const inferred = Boolean(edge.inferred);
    const data = edge.kind === 'data';
    const returnsValue = edge.kind === 'call' && (edge.resultUsed === 'all' || edge.resultUsed === 'some');
    const unknown = edge.kind === 'call' && edge.resultUsed === 'unknown';
    const color = inferred ? 'var(--warning)' : data ? 'var(--success)' : returnsValue ? 'var(--accent)' : 'var(--edge-call)';
    const dash = inferred || data || unknown ? '6 4' : undefined;
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
      data: { edge, dimmed, highlighted },
      style: {
        stroke: highlighted ? 'var(--accent)' : color,
        strokeWidth: highlighted ? 2.6 : 1.5,
        strokeDasharray: dash,
        opacity: dimmed ? 0.1 : 0.9,
      },
      markerEnd: {
        type: 'arrowclosed',
        color: highlighted ? 'var(--accent)' : color,
        width: 16,
        height: 16,
      },
      markerStart: returnsValue
        ? {
            type: 'arrowclosed',
            color: highlighted ? 'var(--accent)' : color,
            width: 14,
            height: 14,
          }
        : undefined,
    } satisfies FlowEdge;
  });
}
