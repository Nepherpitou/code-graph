import {
  Background,
  BackgroundVariant,
  Controls,
  Panel,
  ReactFlow,
  type EdgeTypes,
  type NodeMouseHandler,
  type NodeTypes,
} from '@xyflow/react';

import type { GraphNode } from '../../types';
import { CanvasMiniMap } from './CanvasMiniMap';
import {
  CULL_THRESHOLD,
  MIN_READABLE_ZOOM,
  type FlowNodeData,
} from './graph';
import { NodeCard } from './NodeCard';
import { LaneEdge } from './LaneEdge';
import type { GraphView } from './useGraphView';

const nodeTypes = { card: NodeCard } as unknown as NodeTypes;
const edgeTypes = { lane: LaneEdge } as unknown as EdgeTypes;

interface Props {
  view: GraphView;
  onContextMenu: (node: GraphNode, x: number, y: number) => void;
}

export function GraphCanvas({ view, onContextMenu }: Props) {
  const onNodeContextMenu: NodeMouseHandler = (event, node) => {
    event.preventDefault();
    const data = node.data as FlowNodeData;
    onContextMenu(data.node, event.clientX, event.clientY);
  };

  return (
    <ReactFlow
      nodes={view.flowNodes}
      edges={view.flowEdges}
      nodeTypes={nodeTypes}
      edgeTypes={edgeTypes}
      fitView={false}
      minZoom={MIN_READABLE_ZOOM}
      maxZoom={2}
      nodesConnectable={false}
      nodesDraggable={false}
      nodesFocusable={false}
      edgesFocusable={false}
      edgesReconnectable={false}
      elevateNodesOnSelect={false}
      autoPanOnNodeFocus={false}
      onlyRenderVisibleElements={view.flowNodes.length > CULL_THRESHOLD}
      colorMode="light"
      onNodeClick={view.onNodeClick}
      onNodeDoubleClick={view.onNodeDoubleClick}
      onNodeContextMenu={onNodeContextMenu}
      onPaneClick={() => view.setSelectedId(null)}
    >
      <Background variant={BackgroundVariant.Dots} gap={22} size={1} color="var(--border)" />
      <CanvasMiniMap nodes={view.flowNodes} />
      <Controls showInteractive={false} />
      <Panel position="top-left" className="flow-status">
        {view.layouting ? <span className="badge accent">layout…</span> : null}
        {view.entryInfo ? (
          <span className="badge accent">
            entry points · {view.entryRoots.length} roots
          </span>
        ) : null}
        {view.focusIds ? (
          <span className="badge accent">
            focus · {view.focusIds.size} nodes
            <button className="chip-button" onClick={view.clearFocus} title="Clear focus">×</button>
          </span>
        ) : null}
      </Panel>
    </ReactFlow>
  );
}
