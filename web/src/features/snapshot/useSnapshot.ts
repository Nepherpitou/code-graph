import { useCallback, useEffect, useState, type Dispatch, type SetStateAction } from 'react';

import {
  fetchAllNodes,
  fetchGraphs,
  fetchGraphSummary,
  fetchOverview,
  fetchStatus,
  refreshAnalysis,
} from '../../lib/api';
import type {
  AnalysisStatus,
  GraphNode,
  GraphReport,
  GraphSummary,
  Overview,
} from '../../types';

export interface SnapshotSlice {
  report: GraphReport | null;
  setReport: Dispatch<SetStateAction<GraphReport | null>>;
  snapshot: GraphSummary | null;
  overview: Overview | null;
  fileOverview: Overview | null;
  allNodes: GraphNode[];
  apiContext: { base: string; graphId: string } | null;
  analysisStatus: AnalysisStatus | null;
  loading: boolean;
  error: string | null;
  setError: (message: string | null) => void;
  loadReport: (data: GraphReport) => void;
  readFile: (file: File) => void;
  loadFileOverview: () => Promise<Overview | null>;
  refreshAnalysis: () => Promise<void>;
}

export function useSnapshot(): SnapshotSlice {
  const [report, setReport] = useState<GraphReport | null>(null);
  const [snapshot, setSnapshot] = useState<GraphSummary | null>(null);
  const [overview, setOverview] = useState<Overview | null>(null);
  const [fileOverview, setFileOverview] = useState<Overview | null>(null);
  const [allNodes, setAllNodes] = useState<GraphNode[]>([]);
  const [apiContext, setApiContext] = useState<{ base: string; graphId: string } | null>(null);
  const [analysisStatus, setAnalysisStatus] = useState<AnalysisStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadReport = useCallback((data: GraphReport) => {
    if (!data || !Array.isArray(data.nodes) || !Array.isArray(data.edges)) {
      throw new Error('JSON is not a code-graph report (nodes/edges missing)');
    }
    setReport(data);
    setSnapshot(null);
    setOverview(null);
    setFileOverview(null);
    setAllNodes(data.nodes);
    setApiContext(null);
    setError(null);
    setLoading(false);
  }, []);

  const loadFromApi = useCallback(async (base: string, graphId: string) => {
    setLoading(true);
    setError(null);
    try {
      const status = await fetchStatus(base);
      setAnalysisStatus(status);
      setApiContext({ base, graphId: status.graphId ? String(status.graphId) : graphId });
      if (status.state === 'running') {
        setReport(null);
        return;
      }
      if (status.state === 'failed') {
        setLoading(false);
        setError(status.error ?? status.message);
        return;
      }
      const resolved = status.graphId ? String(status.graphId) : graphId;
      const summary = await fetchGraphSummary(base, resolved);
      const [nextOverview, nodes] = await Promise.all([
        fetchOverview(base, resolved, 'dir'),
        fetchAllNodes(base, resolved, Math.max(1, summary.stats.nodes)),
      ]);
      setSnapshot(summary);
      setOverview(nextOverview);
      setFileOverview(null);
      setAllNodes(nodes);
      setReport(null);
      setApiContext({ base, graphId: String(summary.id) });
      setLoading(false);
    } catch (caught) {
      setLoading(false);
      setError(`API load failed: ${(caught as Error).message}`);
    }
  }, []);

  const loadGraphJson = useCallback(async (url: string) => {
    setLoading(true);
    setError(null);
    try {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      loadReport(await response.json() as GraphReport);
    } catch (caught) {
      setLoading(false);
      if (url === 'graph.json') {
        setError('No graph.json beside the app. Build one with `cargo run -- analyze <repo> -o web/public/graph.json`, then open it here.');
      } else {
        setError(`Failed to load ${url}: ${(caught as Error).message}`);
      }
    }
  }, [loadReport]);

  const loadFileOverview = useCallback(async () => {
    if (!apiContext) return null;
    if (fileOverview) return fileOverview;
    try {
      const next = await fetchOverview(apiContext.base, apiContext.graphId, 'file');
      setFileOverview(next);
      return next;
    } catch (caught) {
      setError(`File overview failed: ${(caught as Error).message}`);
      return null;
    }
  }, [apiContext, fileOverview]);

  const refresh = useCallback(async () => {
    if (!apiContext) return;
    try {
      setError(null);
      const status = await refreshAnalysis(apiContext.base);
      setAnalysisStatus(status);
    } catch (caught) {
      setError(`Refresh failed: ${(caught as Error).message}`);
    }
  }, [apiContext]);

  const readFile = useCallback((file: File) => {
    const reader = new FileReader();
    reader.onload = () => {
      try {
        loadReport(JSON.parse(String(reader.result)) as GraphReport);
      } catch (caught) {
        setError((caught as Error).message);
      }
    };
    reader.readAsText(file);
  }, [loadReport]);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const apiParam = params.get('api');
    if (apiParam !== null) {
      const base = apiParam && apiParam !== '1' ? apiParam : '/api/v1';
      void loadFromApi(base, params.get('id') ?? 'latest');
      return;
    }
    if (!params.get('graph')) {
      void (async () => {
        try {
          const status = await fetchStatus('/api/v1');
          if (status.state !== 'idle') {
            await loadFromApi('/api/v1', status.graphId ? String(status.graphId) : 'latest');
            return;
          }
          const graphs = await fetchGraphs('/api/v1');
          if (graphs.length > 0) {
            await loadFromApi('/api/v1', 'latest');
            return;
          }
        } catch {
          // No API server; fall through to the static graph bundle.
        }
        void loadGraphJson('graph.json');
      })();
      return;
    }
    void loadGraphJson(params.get('graph') ?? 'graph.json');
  }, [loadFromApi, loadGraphJson]);

  useEffect(() => {
    if (!apiContext || analysisStatus?.state !== 'running') return;
    const timer = window.setInterval(() => {
      void (async () => {
        try {
          const status = await fetchStatus(apiContext.base);
          setAnalysisStatus(status);
          if (status.state !== 'running') {
            await loadFromApi(apiContext.base, status.graphId ? String(status.graphId) : 'latest');
          }
        } catch (caught) {
          setError(`Status check failed: ${(caught as Error).message}`);
        }
      })();
    }, 700);
    return () => window.clearInterval(timer);
  }, [apiContext, analysisStatus?.state, loadFromApi]);

  return {
    report,
    setReport,
    snapshot,
    overview,
    fileOverview,
    allNodes,
    apiContext,
    analysisStatus,
    loading,
    error,
    setError,
    loadReport,
    readFile,
    loadFileOverview,
    refreshAnalysis: refresh,
  };
}
