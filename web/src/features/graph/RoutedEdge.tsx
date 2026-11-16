import { useMemo } from 'react';
import {
  BaseEdge,
  EdgeLabelRenderer,
  getSmoothStepPath,
  type Edge,
  type EdgeProps,
} from '@xyflow/react';

import type { EdgeObstacle, FlowEdgeData } from './graph';

const CELL = 24;
const MARGIN = 22;
const PAD = 48;

interface Point { x: number; y: number }

function hitsObstacle(x: number, y: number, obstacles: EdgeObstacle[]): boolean {
  return obstacles.some((box) =>
    x >= box.x - MARGIN
    && x <= box.x + box.width + MARGIN
    && y >= box.y - MARGIN
    && y <= box.y + box.height + MARGIN,
  );
}

function simplify(points: Point[]): Point[] {
  const compact: Point[] = [];
  for (const point of points) {
    const previous = compact[compact.length - 1];
    if (!previous || previous.x !== point.x || previous.y !== point.y) compact.push(point);
  }
  if (compact.length < 3) return compact;
  const result: Point[] = [compact[0]];
  for (let index = 1; index < compact.length - 1; index += 1) {
    const previous = result[result.length - 1];
    const current = compact[index];
    const next = compact[index + 1];
    const horizontal = previous.y === current.y && current.y === next.y;
    const vertical = previous.x === current.x && current.x === next.x;
    if (!horizontal && !vertical) result.push(current);
  }
  result.push(compact[compact.length - 1]);
  return result;
}

function routeAround(
  source: Point,
  target: Point,
  obstacles: EdgeObstacle[],
): Point[] | null {
  const minX = Math.min(source.x, target.x, ...obstacles.map((box) => box.x)) - PAD;
  const minY = Math.min(source.y, target.y, ...obstacles.map((box) => box.y)) - PAD;
  const maxX = Math.max(source.x, target.x, ...obstacles.map((box) => box.x + box.width)) + PAD;
  const maxY = Math.max(source.y, target.y, ...obstacles.map((box) => box.y + box.height)) + PAD;
  const columns = Math.max(2, Math.ceil((maxX - minX) / CELL));
  const rows = Math.max(2, Math.ceil((maxY - minY) / CELL));
  const total = columns * rows;
  const index = (column: number, row: number): number => row * columns + column;
  const point = (column: number, row: number): Point => ({
    x: minX + (column + 0.5) * CELL,
    y: minY + (row + 0.5) * CELL,
  });
  const startColumn = Math.min(columns - 1, Math.max(0, Math.floor((source.x - minX) / CELL)));
  const startRow = Math.min(rows - 1, Math.max(0, Math.floor((source.y - minY) / CELL)));
  const endColumn = Math.min(columns - 1, Math.max(0, Math.floor((target.x - minX) / CELL)));
  const endRow = Math.min(rows - 1, Math.max(0, Math.floor((target.y - minY) / CELL)));
  const blocked = new Uint8Array(total);
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const center = point(column, row);
      if (hitsObstacle(center.x, center.y, obstacles)) blocked[index(column, row)] = 1;
    }
  }
  const start = index(startColumn, startRow);
  const end = index(endColumn, endRow);
  // Dense focus slices often put another card on the handle's grid cell. The
  // handle itself belongs to the excluded source/target node, so open it.
  blocked[start] = 0;
  blocked[end] = 0;

  const parent = new Int32Array(total).fill(-1);
  const visited = new Uint8Array(total);
  const queue = new Int32Array(total);
  let head = 0;
  let tail = 0;
  queue[tail] = start;
  tail += 1;
  visited[start] = 1;
  const directions = [[1, 0], [-1, 0], [0, 1], [0, -1]];
  while (head < tail) {
    const current = queue[head];
    head += 1;
    if (current === end) break;
    const column = current % columns;
    const row = Math.floor(current / columns);
    for (const [dx, dy] of directions) {
      const nextColumn = column + dx;
      const nextRow = row + dy;
      if (nextColumn < 0 || nextColumn >= columns || nextRow < 0 || nextRow >= rows) continue;
      const next = index(nextColumn, nextRow);
      if (visited[next] || blocked[next]) continue;
      visited[next] = 1;
      parent[next] = current;
      queue[tail] = next;
      tail += 1;
    }
  }
  if (!visited[end]) return null;

  const cells: Point[] = [];
  let cursor = end;
  while (cursor !== -1) {
    const column = cursor % columns;
    const row = Math.floor(cursor / columns);
    cells.push(point(column, row));
    if (cursor === start) break;
    cursor = parent[cursor];
  }
  cells.reverse();
  return simplify([source, ...cells, target]);
}

function pathFromPoints(points: Point[]): string {
  return points.map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x},${point.y}`).join(' ');
}

export function RoutedEdge({
  sourceX,
  sourceY,
  sourcePosition,
  targetX,
  targetY,
  targetPosition,
  source,
  target,
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
    const obstacles = (data?.obstacles ?? []).filter((box) => box.id !== source && box.id !== target);
    const sourcePoint = { x: sourceX, y: sourceY };
    const targetPoint = { x: targetX, y: targetY };
    if (obstacles.length > 0) {
      const points = routeAround(sourcePoint, targetPoint, obstacles);
      if (points) {
        const middle = points[Math.floor(points.length / 2)];
        return { path: pathFromPoints(points), labelX: middle.x, labelY: middle.y };
      }
    }
    const [path, labelX, labelY] = getSmoothStepPath({
      sourceX,
      sourceY,
      sourcePosition,
      targetX,
      targetY,
      targetPosition,
      borderRadius: 12,
      offset: 24,
    });
    return { path, labelX, labelY };
  }, [data?.obstacles, source, sourcePosition, sourceX, sourceY, target, targetPosition, targetX, targetY]);

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
            <span style={{ background: labelBgStyle?.fill ?? 'var(--panel)', padding: `${labelBgPadding?.[1] ?? 2}px ${labelBgPadding?.[0] ?? 4}px`, borderRadius: labelBgBorderRadius ?? 4 }}>
              {label}
            </span>
          </div>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
}
