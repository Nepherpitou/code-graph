import ELK from 'elkjs/lib/elk.bundled.js';

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

const elk = new ELK();

export async function computeLayout(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  kind: LayoutKind,
): Promise<Record<string, { x: number; y: number }>> {
  if (nodes.length === 0) return {};
  if (kind === 'grid') {
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

  const graph = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': kind === 'stress' ? 'stress' : 'layered',
      // elkjs 0.12 lays layers along RIGHT as a top-to-bottom flow for this graph.
      'elk.direction': 'RIGHT',
      'elk.separateConnectedComponents': 'true',
      'elk.spacing.componentSpacing': '60',
      'elk.spacing.nodeNode': '32',
      'elk.layered.spacing.nodeNodeBetweenLayers': '78',
      'elk.layered.nodePlacement.strategy': 'NETWORK_SIMPLEX',
      'elk.layered.cycleBreaking.strategy': 'GREEDY',
      'elk.stress.desiredEdgeLength': '150',
      'elk.aspectRatio': '1.6',
      'elk.edgeRouting': 'SPLINES',
    },
    children: nodes.map((node) => ({
      id: node.id,
      width: node.width,
      height: node.height,
    })),
    edges: edges.map((edge) => ({ id: edge.id, sources: [edge.source], targets: [edge.target] })),
  };

  const result = await elk.layout(graph);
  const positions: Record<string, { x: number; y: number }> = {};
  for (const child of result.children ?? []) {
    positions[child.id] = { x: child.x ?? 0, y: child.y ?? 0 };
  }
  return positions;
}
