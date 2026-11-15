export interface LspPoint { line: number; character: number }
export interface LspRange { start: LspPoint; end: LspPoint }

export interface CallSite {
  file: string;
  line: number;
  character: number;
  resultUsed: boolean | null;
}

export interface NodeMetrics {
  fanIn: number;
  fanOut: number;
  callIn: number;
  callOut: number;
  transitiveFanIn: number;
  transitiveFanOut: number;
  depth: number;
  height: number;
  scc: number;
  sccSize: number;
  inCycle: boolean;
  pageRank: number;
  betweenness: number;
  instability: number;
  resultIgnoredRatio: number;
  consumesResults: number;
  providesResults: number;
  externalFanOut: number;
  constantsUsed: number;
  moduleRefs: number;
  ifc: number;
  deadCodeCandidate: boolean;
  hotspot: number;
}

export type NodeKind = 'function' | 'method' | 'constructor' | 'constant' | 'variable' | 'other';

export interface GraphNode {
  id: string;
  name: string;
  qname: string;
  kind: NodeKind;
  file: string;
  line: number;
  endLine: number;
  loc: number;
  params: number;
  cyclomatic: number;
  exported: boolean;
  external: boolean;
  lazy: boolean;
  synthetic: boolean;
  isEntryLike: boolean;
  declaration: string;
  metrics: NodeMetrics;
}

export type ResultUsage = 'none' | 'some' | 'all' | 'unknown';

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
  kind: 'call' | 'data';
  sites: number;
  resultUsed: ResultUsage;
  external: boolean;
  inferred?: boolean;
  callSites: CallSite[];
}

export interface FileSummary {
  file: string;
  nodes: number;
  callIn: number;
  callOut: number;
  constantsUsed: number;
  deadCode: number;
}

export interface GraphStats {
  nodes: number;
  functions: number;
  constants: number;
  edges: number;
  dataEdges: number;
  externalEdges: number;
  density: number;
  avgFanIn: number;
  avgFanOut: number;
  maxFanIn: number;
  maxFanOut: number;
  cycles: number;
  nodesInCycles: number;
  largestCycle: number;
  deadCode: number;
  isolated: number;
  maxHeight: number;
  maxDepth: number;
  crossFileEdges: number;
  crossFileRatio: number;
  resultIgnoredRatio: number;
}

export interface TopEntry { id: string; name: string; file: string; value: number }

export interface AnalysisStatus {
  state: 'idle' | 'running' | 'ready' | 'failed';
  phase: string;
  message: string;
  startedAt: string | null;
  finishedAt: string | null;
  graphId: number | null;
  error: string | null;
  canAnalyze: boolean;
}

export interface GraphSummary {
  id: number;
  createdAt: string;
  meta: GraphReport['meta'];
  stats: GraphStats;
  top: GraphReport['top'];
  warnings: string[];
}

export interface GraphSlice {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface FocusSlice extends GraphSlice {
  entry: string | null;
  exit: string | null;
  route: string[];
}

export interface FileSlice extends GraphSlice {
  total: number;
  truncated: boolean;
}

export interface EntryPoint {
  id: string;
  name: string;
  file: string;
  reach: number;
  height: number;
  fanOut: number;
}

export interface EntryGraph extends GraphSlice {
  roots: EntryPoint[];
  totalRoots: number;
  depth: number;
  truncated: boolean;
}

export interface OverviewNode {
  id: string;
  label: string;
  kind: 'file' | 'dir';
  path: string;
  nodes: number;
  callables: number;
  constants: number;
  cycleNodes: number;
  deadCode: number;
  hotspot: number;
}

export interface OverviewEdge {
  id: string;
  source: string;
  target: string;
  calls: number;
  data: number;
  total: number;
}

export interface Overview {
  group: 'file' | 'dir';
  totalNodes: number;
  nodes: OverviewNode[];
  edges: OverviewEdge[];
}

export interface PathResult {
  found: boolean;
  path: string[];
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface GraphReport {
  meta: {
    tool: string;
    version: string;
    generatedAt: string;
    root: string;
    language: string;
    server: string;
    durationMs: number;
    files: number;
    symbols: number;
    nodeCount: number;
    edgeCount: number;
    truncated: boolean;
    options: Record<string, unknown>;
  };
  nodes: GraphNode[];
  edges: GraphEdge[];
  files: FileSummary[];
  stats: GraphStats;
  top: {
    fanIn: TopEntry[];
    fanOut: TopEntry[];
    pageRank: TopEntry[];
    betweenness: TopEntry[];
    hotspot: TopEntry[];
    complexity: TopEntry[];
    ifc: TopEntry[];
    deadCode: TopEntry[];
  };
  warnings: string[];
}
