import { useMemo } from 'react';
import {
  BaseEdge,
  EdgeLabelRenderer,
  type Edge,
  type EdgeProps,
} from '@xyflow/react';

import type { FlowEdgeData } from './graph';

interface Point { x: number; y: number }

const LANE_STEP = 14;
const TURN = 8;

function roundedPath(points: Point[]): string {
  if (points.length < 2) return '';
  let path = `M ${points[0].x},${points[0].y}`;
  for (let index = 1; index < points.length - 1; index += 1) {
    const previous = points[index - 1];
    const current = points[index];
    const next = points[index + 1];
    const inX = current.x === previous.x ? 0 : Math.sign(current.x - previous.x);
    const inY = current.y === previous.y ? 0 : Math.sign(current.y - previous.y);
    const outX = next.x === current.x ? 0 : Math.sign(next.x - current.x);
    const outY = next.y === current.y ? 0 : Math.sign(next.y - current.y);
    path += ` L ${current.x - inX * TURN},${current.y - inY * TURN}`;
    path += ` Q ${current.x},${current.y} ${current.x + outX * TURN},${current.y + outY * TURN}`;
  }
  const last = points[points.length - 1];
  path += ` L ${last.x},${last.y}`;
  return path;
}

function pathPoints(
  sourceX: number,
  sourceY: number,
  targetX: number,
  targetY: number,
  sourcePosition: string,
  targetPosition: string,
  lane: number,
): Point[] {
  const offset = lane * LANE_STEP;
  const source = { x: sourceX, y: sourceY };
  const target = { x: targetX, y: targetY };
  if (sourcePosition === 'right' && targetPosition === 'left') {
    const laneX = Math.max(sourceX + 22, sourceX + 64 + offset);
    return [source, { x: laneX, y: sourceY }, { x: laneX, y: targetY }, target];
  }
  if (sourcePosition === 'left' && targetPosition === 'right') {
    const laneX = Math.min(sourceX - 22, sourceX - 64 + offset);
    return [source, { x: laneX, y: sourceY }, { x: laneX, y: targetY }, target];
  }
  if (sourcePosition === 'bottom' && targetPosition === 'top') {
    const laneY = Math.max(sourceY + 22, sourceY + 64 + offset);
    return [source, { x: sourceX, y: laneY }, { x: targetX, y: laneY }, target];
  }
  if (sourcePosition === 'top' && targetPosition === 'bottom') {
    const laneY = Math.min(sourceY - 22, sourceY - 64 + offset);
    return [source, { x: sourceX, y: laneY }, { x: targetX, y: laneY }, target];
  }
  return [source, target];
}

export function LaneEdge({
  sourceX,
  sourceY,
  sourcePosition,
  targetX,
  targetY,
  targetPosition,
  data,
  style,
  markerEnd,
  markerStart,
  interactionWidth,
  label,
  labelStyle,
  labelBgStyle,
  labelBgPadding,
  labelBgBorderRadius,
}: EdgeProps<Edge<FlowEdgeData>>) {
  const routing = useMemo(() => {
    const points = pathPoints(
      sourceX,
      sourceY,
      targetX,
      targetY,
      sourcePosition,
      targetPosition,
      data?.lane ?? 0,
    );
    const middle = points[Math.floor(points.length / 2)];
    return { path: roundedPath(points), labelX: middle.x, labelY: middle.y };
  }, [data?.lane, sourcePosition, sourceX, sourceY, targetPosition, targetX, targetY]);

  return (
    <>
      <BaseEdge
        path={routing.path}
        markerEnd={markerEnd}
        markerStart={markerStart}
        interactionWidth={interactionWidth}
        style={style}
      />
      {label ? (
        <EdgeLabelRenderer>
          <div
            className="edge-site-label"
            style={{
              position: 'absolute',
              transform: `translate(-50%, -50%) translate(${routing.labelX}px, ${routing.labelY}px)`,
              ...labelStyle,
            }}
          >
            <span style={{
              background: labelBgStyle?.fill ?? 'var(--panel)',
              padding: `${labelBgPadding?.[1] ?? 2}px ${labelBgPadding?.[0] ?? 4}px`,
              borderRadius: labelBgBorderRadius ?? 4,
            }}>
              {label}
            </span>
          </div>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
}
