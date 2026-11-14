import { useEffect, useMemo, useRef, type PointerEvent } from 'react';
import {
  getNodesBounds,
  useReactFlow,
  useStore,
  useViewport,
  type Node,
} from '@xyflow/react';

import { KIND_COLORS, type FlowNodeData } from './graph';

const WIDTH = 180;
const HEIGHT = 120;
const PADDING = 6;

interface Props {
  nodes: Node<FlowNodeData, 'card'>[];
}

interface Projection {
  scale: number;
  offsetX: number;
  offsetY: number;
  minX: number;
  minY: number;
}

function project(bounds: { x: number; y: number; width: number; height: number }): Projection {
  const width = Math.max(bounds.width, 1);
  const height = Math.max(bounds.height, 1);
  const scale = Math.min((WIDTH - PADDING * 2) / width, (HEIGHT - PADDING * 2) / height);
  return {
    scale,
    offsetX: (WIDTH - width * scale) / 2 - bounds.x * scale,
    offsetY: (HEIGHT - height * scale) / 2 - bounds.y * scale,
    minX: bounds.x,
    minY: bounds.y,
  };
}

function point(projection: Projection, x: number, y: number): [number, number] {
  return [projection.offsetX + x * projection.scale, projection.offsetY + y * projection.scale];
}

export function CanvasMiniMap({ nodes }: Props) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const staticRef = useRef<HTMLCanvasElement | null>(null);
  const projectionRef = useRef<Projection>(project({ x: 0, y: 0, width: 1, height: 1 }));
  const draggingRef = useRef(false);
  const { setCenter, getViewport } = useReactFlow();
  const viewport = useViewport();
  const flowWidth = useStore((state) => state.width);
  const flowHeight = useStore((state) => state.height);
  const bounds = useMemo(() => getNodesBounds(nodes), [nodes]);
  const projection = useMemo(() => project(bounds), [bounds]);
  projectionRef.current = projection;

  useEffect(() => {
    const dpr = window.devicePixelRatio || 1;
    const canvas = staticRef.current ?? document.createElement('canvas');
    staticRef.current = canvas;
    canvas.width = WIDTH * dpr;
    canvas.height = HEIGHT * dpr;
    const context = canvas.getContext('2d');
    if (!context) return;
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
    context.clearRect(0, 0, WIDTH, HEIGHT);
    for (const node of nodes) {
      const data = node.data as FlowNodeData;
      const [x, y] = point(projection, node.position.x, node.position.y);
      const width = Math.max((node.width ?? 0) * projection.scale, 1.5);
      const height = Math.max((node.height ?? 0) * projection.scale, 1.5);
      context.fillStyle = KIND_COLORS[data.node.kind] ?? '#64748b';
      context.fillRect(x, y, width, height);
    }
  }, [nodes, projection]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== WIDTH * dpr || canvas.height !== HEIGHT * dpr) {
      canvas.width = WIDTH * dpr;
      canvas.height = HEIGHT * dpr;
    }
    const context = canvas.getContext('2d');
    if (!context) return;
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
    context.clearRect(0, 0, WIDTH, HEIGHT);
    if (staticRef.current) context.drawImage(staticRef.current, 0, 0, WIDTH, HEIGHT);

    const zoom = viewport.zoom || 1;
    const [left, top] = point(projection, -viewport.x / zoom, -viewport.y / zoom);
    const width = (flowWidth / zoom) * projection.scale;
    const height = (flowHeight / zoom) * projection.scale;
    context.fillStyle = 'rgba(88, 166, 255, 0.12)';
    context.fillRect(left, top, width, height);
    context.strokeStyle = 'rgba(88, 166, 255, 0.9)';
    context.lineWidth = 1;
    context.strokeRect(left, top, width, height);
  }, [viewport, flowWidth, flowHeight, projection]);

  const moveTo = (event: PointerEvent<HTMLCanvasElement>): void => {
    const canvas = event.currentTarget;
    const box = canvas.getBoundingClientRect();
    const localX = event.clientX - box.left;
    const localY = event.clientY - box.top;
    const { scale, offsetX, offsetY } = projectionRef.current;
    const graphX = (localX - offsetX) / scale;
    const graphY = (localY - offsetY) / scale;
    void setCenter(graphX, graphY, { zoom: getViewport().zoom, duration: 0 });
  };

  return (
    <canvas
      ref={canvasRef}
      className="canvas-minimap"
      style={{ width: `${WIDTH}px`, height: `${HEIGHT}px` }}
      role="button"
      tabIndex={0}
      aria-label="Graph minimap"
      title="Click or drag to move the viewport"
      onPointerDown={(event) => {
        draggingRef.current = true;
        event.currentTarget.setPointerCapture(event.pointerId);
        moveTo(event);
      }}
      onPointerMove={(event) => {
        if (draggingRef.current) moveTo(event);
      }}
      onPointerUp={(event) => {
        draggingRef.current = false;
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onPointerCancel={() => {
        draggingRef.current = false;
      }}
    />
  );
}
