import ELK from 'elkjs/lib/elk-api';
import ElkWorker from 'elkjs/lib/elk-worker.min.js?worker';

export type LayoutKind = 'layered' | 'stress' | 'grid';

export interface LayoutNode {
  id: string;
  width: number;
  height: number;
}

export interface LayoutEdge {
  id: string;
  source: string;
  target: string;
}

type ElkInstance = InstanceType<typeof ELK>;

let activeElk: ElkInstance | null = null;
let layoutGeneration = 0;
const layoutCache = new Map<string, Record<string, { x: number; y: number }>>();
const MAX_LAYOUT_CACHE = 8;

function createElk(): ElkInstance {
  return new ELK({ workerFactory: () => new ElkWorker() });
}

function elk(): ElkInstance {
  activeElk ??= createElk();
  return activeElk;
}

/**
 * Drop the current layout worker. Any in-flight `computeLayout` resolves to
 * `null`; the next call gets a fresh worker.
 */
export function cancelLayout(): void {
  layoutGeneration += 1;
  activeElk?.terminateWorker();
  activeElk = null;
}

function cacheKey(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  kind: LayoutKind,
  scope: string,
): string {
  return `${scope}|${kind}|${nodes.map((node) => node.id).join('|')}|${edges
    .map((edge) => edge.id)
    .join('|')}`;
}

function gridPositions(nodes: LayoutNode[]): Record<string, { x: number; y: number }> {
  const sorted = [...nodes].sort((a, b) => a.id.localeCompare(b.id));
  const columns = Math.max(1, Math.ceil(Math.sqrt(sorted.length * 1.4)));
  const positions: Record<string, { x: number; y: number }> = {};
  const colWidth = Math.max(...nodes.map((node) => node.width)) + 42;
  const rowHeight = Math.max(...nodes.map((node) => node.height)) + 30;
  sorted.forEach((node, index) => {
    positions[node.id] = {
      x: (index % columns) * colWidth,
      y: Math.floor(index / columns) * rowHeight,
    };
  });
  return positions;
}

function canonicalNodes(nodes: LayoutNode[]): LayoutNode[] {
  return [...nodes].sort((left, right) => left.id < right.id ? -1 : left.id > right.id ? 1 : 0);
}

function canonicalEdges(edges: LayoutEdge[]): LayoutEdge[] {
  return [...edges].sort((left, right) => left.source.localeCompare(right.source)
    || left.target.localeCompare(right.target)
    || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0));
}

function layoutOptions(kind: LayoutKind): Record<string, string> {
  return {
    'elk.algorithm': kind === 'stress' ? 'stress' : 'layered',
    // elkjs 0.12 lays layers along RIGHT as a top-to-bottom flow for this graph.
    'elk.direction': 'RIGHT',
    'elk.separateConnectedComponents': 'true',
    'elk.spacing.componentSpacing': '60',
    'elk.spacing.nodeNode': '32',
    'elk.layered.spacing.nodeNodeBetweenLayers': '78',
    // BRANDES_KOEPF is markedly faster than NETWORK_SIMPLEX on sparse call
    // graphs. It produces a taller drawing, which is acceptable because the
    // canvas is panned rather than fitted wholesale.
    'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
    'elk.layered.cycleBreaking.strategy': 'GREEDY',
    'elk.stress.desiredEdgeLength': '150',
    'elk.aspectRatio': '1.6',
    'elk.edgeRouting': 'SPLINES',
  };
}

function elkGraph(nodes: LayoutNode[], edges: LayoutEdge[], kind: LayoutKind) {
  return {
    id: 'root',
    layoutOptions: layoutOptions(kind),
    children: nodes.map((node) => ({
      id: node.id,
      width: node.width,
      height: node.height,
    })),
    edges: edges.map((edge) => ({ id: edge.id, sources: [edge.source], targets: [edge.target] })),
  };
}

interface ElkLayoutResult {
  children?: Array<{ id: string; x?: number; y?: number }>;
}

function elkResultPositions(result: ElkLayoutResult): Record<string, { x: number; y: number }> {
  const positions: Record<string, { x: number; y: number }> = {};
  for (const child of result.children ?? []) {
    positions[child.id] = { x: child.x ?? 0, y: child.y ?? 0 };
  }
  return positions;
}

function cacheLayout(key: string, positions: Record<string, { x: number; y: number }>): void {
  layoutCache.set(key, positions);
  if (layoutCache.size <= MAX_LAYOUT_CACHE) return;
  const oldest = layoutCache.keys().next().value;
  if (oldest) layoutCache.delete(oldest);
}

function discardFailedWorker(instance: ElkInstance): void {
  // A worker that failed is unusable for the next request.
  if (activeElk !== instance) return;
  activeElk.terminateWorker();
  activeElk = null;
}

async function elkLayout(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  kind: LayoutKind,
  scope: string,
): Promise<Record<string, { x: number; y: number }> | null> {
  // Canonicalize before both cacheKey and ELK. The API may return the same
  // slice in a different order, which otherwise changes layering and positions.
  const orderedNodes = canonicalNodes(nodes);
  const orderedEdges = canonicalEdges(edges);
  const key = cacheKey(orderedNodes, orderedEdges, kind, scope);
  const cached = layoutCache.get(key);
  if (cached) return cached;
  const generation = layoutGeneration;
  const instance = elk();
  try {
    const result = await instance.layout(elkGraph(orderedNodes, orderedEdges, kind));
    if (generation !== layoutGeneration) return null;
    const positions = elkResultPositions(result);
    cacheLayout(key, positions);
    return positions;
  } catch (error) {
    if (generation !== layoutGeneration) return null;
    discardFailedWorker(instance);
    throw error;
  }
}

export async function computeLayout(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  kind: LayoutKind,
  scope = '',
): Promise<Record<string, { x: number; y: number }> | null> {
  if (nodes.length === 0) return {};
  if (kind === 'grid') return gridPositions(nodes);
  return elkLayout(nodes, edges, kind, scope);
}
