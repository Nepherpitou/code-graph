import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  Panel,
  ReactFlow,
  type Node,
  type NodeMouseHandler,
  type NodeTypes,
} from '@xyflow/react';

import type { GraphNode } from '../../types';
import { KIND_COLORS, type FlowNodeData } from './graph';
import { NodeCard } from './NodeCard';
import type { GraphView } from './useGraphView';

const nodeTypes = { card: NodeCard } as unknown as NodeTypes;

function miniMapColor(node: Node): string {
  // MiniMap drops our node generic; every rendered node came from FlowNodeData.
  const data = node.data as FlowNodeData;
  return KIND_COLORS[data.node.kind] ?? '#64748b';
}

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
      fitView={false}
      minZoom={0.05}
      maxZoom={2}
      nodesConnectable={false}
      colorMode="system"
      onNodeClick={view.onNodeClick}
      onNodeDoubleClick={view.onNodeDoubleClick}
      onNodeContextMenu={onNodeContextMenu}
      onPaneClick={() => view.setSelectedId(null)}
    >
      <Background variant={BackgroundVariant.Dots} gap={22} size={1} color="var(--border)" />
      <MiniMap
        pannable
        zoomable
        nodeColor={miniMapColor}
        maskColor="color-mix(in srgb, var(--bg) 70%, transparent)"
      />
      <Controls showInteractive={false} />
      <Panel position="top-left" className="flow-status">
        {view.layouting ? <span className="badge accent">layout…</span> : null}
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
