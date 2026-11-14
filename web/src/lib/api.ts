import type {
  AnalysisStatus,
  FileSlice,
  GraphNode,
  GraphSlice,
  GraphSummary,
  Overview,
  PathResult,
} from '../types';

function normalizeBase(base: string): string {
  return base.replace(/\/+$/, '');
}

async function getJson<T>(url: string): Promise<T> {
  const response = await fetch(url);
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as { error?: string } | null;
    throw new Error(body?.error ?? `${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<T>;
}

export async function fetchGraphs(base: string): Promise<GraphSummary[]> {
  const result = await getJson<{ graphs: GraphSummary[] }>(`${normalizeBase(base)}/graphs`);
  return result.graphs;
}

export async function fetchStatus(base: string): Promise<AnalysisStatus> {
  return getJson<AnalysisStatus>(`${normalizeBase(base)}/status`);
}

export async function fetchGraphSummary(base: string, graphId: string): Promise<GraphSummary> {
  return getJson<GraphSummary>(`${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}`);
}

export async function fetchBootstrap(base: string, graphId: string, limit = 200): Promise<GraphSlice> {
  return getJson<GraphSlice>(
    `${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}/bootstrap?limit=${limit}`,
  );
}

export async function fetchOverview(
  base: string,
  graphId: string,
  group: 'file' | 'dir' = 'dir',
): Promise<Overview> {
  return getJson<Overview>(
    `${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}/overview?group=${group}`,
  );
}

export async function fetchFileSlice(
  base: string,
  graphId: string,
  path: string,
  group: 'file' | 'dir',
  limit = 500,
): Promise<FileSlice> {
  const params = new URLSearchParams({ path, group, limit: String(limit) });
  return getJson<FileSlice>(
    `${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}/file?${params}`,
  );
}

export async function fetchAllNodes(
  base: string,
  graphId: string,
  limit = 20_000,
): Promise<GraphNode[]> {
  return getJson<GraphNode[]>(
    `${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}/nodes?limit=${limit}`,
  );
}

export async function searchNodes(
  base: string,
  graphId: string,
  query: string,
  limit = 20,
): Promise<GraphNode[]> {
  const params = new URLSearchParams({ q: query, limit: String(limit) });
  const result = await getJson<GraphSlice>(
    `${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}/search?${params}`,
  );
  return result.nodes;
}

export async function fetchSubgraph(
  base: string,
  graphId: string,
  roots: string[],
  depth = 1,
  limit = 500,
): Promise<GraphSlice> {
  const params = new URLSearchParams({
    roots: roots.join(','),
    depth: String(depth),
    limit: String(limit),
  });
  return getJson<GraphSlice>(
    `${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}/subgraph?${params}`,
  );
}

export async function fetchPath(
  base: string,
  graphId: string,
  from: string,
  to: string,
  maxDepth = 12,
): Promise<PathResult> {
  const params = new URLSearchParams({ from, to, maxDepth: String(maxDepth) });
  return getJson<PathResult>(
    `${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}/path?${params}`,
  );
}

export async function fetchImpact(
  base: string,
  graphId: string,
  node: string,
  depth = 2,
  direction: 'in' | 'out' = 'in',
): Promise<GraphSlice> {
  const params = new URLSearchParams({ node, depth: String(depth), direction });
  return getJson<GraphSlice>(
    `${normalizeBase(base)}/graphs/${encodeURIComponent(graphId)}/impact?${params}`,
  );
}
