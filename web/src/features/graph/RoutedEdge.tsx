import { useMemo } from 'react';
import {
  BaseEdge,
  EdgeLabelRenderer,
  getSmoothStepPath,
  type Edge,
  type EdgeProps,
} from '@xyflow/react';

import type { EdgeObstacle, FlowEdgeData } from './graph';

const CELL = 8;
const MARGIN = 8;
const PAD = 48;
const APPROACH = 28;
const HANDLE_GAP = 5;

interface Point { x: number; y: number }

function approach(point: Point, position: string, distance: number): Point {
  switch (position) {
    case 'top': return { x: point.x, y: point.y - distance };
    case 'bottom': return { x: point.x, y: point.y + distance };
    case 'left': return { x: point.x - distance, y: point.y };
    case 'right': return { x: point.x + distance, y: point.y };
    default: return { x: point.x, y: point.y + distance };
  }
}

function hitsObstacle(x: number, y: number, obstacles: EdgeObstacle[]): boolean {
  return obstacles.some((box) =>
    x >= box.x - MARGIN
    && x <= box.x + box.width + MARGIN
    && y >= box.y - MARGIN
    && y <= box.y + box.height + MARGIN,
  );
}

function compactPoints(points: Point[]): Point[] {
  const compact: Point[] = [];
  for (const point of points) {
    const previous = compact[compact.length - 1];
    if (!previous || previous.x !== point.x || previous.y !== point.y) compact.push(point);
  }
  return compact;
}

function removeCollinear(compact: Point[]): Point[] {
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

function simplify(points: Point[]): Point[] {
  const compact = compactPoints(points);
  return compact.length < 3 ? compact : removeCollinear(compact);
}

function clearApproach(
  point: Point,
  position: string,
  distance: number,
  obstacles: EdgeObstacle[],
): Point {
  let candidate = approach(point, position, distance);
  for (let step = 0; step < 12 && hitsObstacle(candidate.x, candidate.y, obstacles); step += 1) {
    candidate = approach(point, position, distance + step * CELL);
  }
  return candidate;
}

function segmentBlocked(start: Point, end: Point, obstacles: EdgeObstacle[]): boolean {
  const length = Math.hypot(end.x - start.x, end.y - start.y);
  const steps = Math.max(1, Math.ceil(length / 4));
  for (let index = 1; index < steps; index += 1) {
    const t = index / steps;
    if (hitsObstacle(start.x + (end.x - start.x) * t, start.y + (end.y - start.y) * t, obstacles)) {
      return true;
    }
  }
  return false;
}

function visibilityPoints(source: Point, target: Point, obstacles: EdgeObstacle[]): Point[] {
  const points: Point[] = [source, target];
  for (const box of obstacles) {
    points.push(
      { x: box.x - MARGIN, y: box.y - MARGIN },
      { x: box.x + box.width + MARGIN, y: box.y - MARGIN },
      { x: box.x - MARGIN, y: box.y + box.height + MARGIN },
      { x: box.x + box.width + MARGIN, y: box.y + box.height + MARGIN },
    );
  }
  return points;
}

function nearestVisibleTarget(distance: number[], visited: Uint8Array): number {
  let current = -1;
  let best = Infinity;
  for (let index = 0; index < distance.length; index += 1) {
    if (!visited[index] && distance[index] < best) {
      best = distance[index];
      current = index;
    }
  }
  return current;
}

function relaxVisibilityEdges(
  current: number,
  points: Point[],
  obstacles: EdgeObstacle[],
  distance: number[],
  previous: number[],
  visited: Uint8Array,
): void {
  visited[current] = 1;
  for (let next = 0; next < points.length; next += 1) {
    if (visited[next] || next === current) continue;
    if (segmentBlocked(points[current], points[next], obstacles)) continue;
    const dx = points[current].x - points[next].x;
    const dy = points[current].y - points[next].y;
    const candidate = distance[current] + Math.hypot(dx, dy);
    if (candidate >= distance[next]) continue;
    distance[next] = candidate;
    previous[next] = current;
  }
}

function traceVisibilityPath(points: Point[], previous: number[]): Point[] {
  const result: Point[] = [];
  let cursor = 1;
  while (cursor !== -1) {
    result.push(points[cursor]);
    if (cursor === 0) break;
    cursor = previous[cursor];
  }
  return simplify(result.reverse());
}

function findVisibilityPath(points: Point[], obstacles: EdgeObstacle[]): Point[] | null {
  const count = points.length;
  const distance = new Array<number>(count).fill(Infinity);
  const previous = new Array<number>(count).fill(-1);
  const visited = new Uint8Array(count);
  distance[0] = 0;
  for (let round = 0; round < count; round += 1) {
    const current = nearestVisibleTarget(distance, visited);
    if (current < 0 || current === 1) break;
    relaxVisibilityEdges(current, points, obstacles, distance, previous, visited);
  }
  if (!Number.isFinite(distance[1])) return null;
  return traceVisibilityPath(points, previous);
}

function visibilityRoute(
  source: Point,
  target: Point,
  obstacles: EdgeObstacle[],
): Point[] | null {
  return findVisibilityPath(visibilityPoints(source, target, obstacles), obstacles);
}

interface RouteGrid {
  columns: number;
  rows: number;
  start: number;
  end: number;
  blocked: Uint8Array;
  point: (column: number, row: number) => Point;
  index: (column: number, row: number) => number;
}

function buildRouteGrid(source: Point, target: Point, obstacles: EdgeObstacle[]): RouteGrid {
  const minX = Math.min(source.x, target.x, ...obstacles.map((box) => box.x)) - PAD;
  const minY = Math.min(source.y, target.y, ...obstacles.map((box) => box.y)) - PAD;
  const maxX = Math.max(source.x, target.x, ...obstacles.map((box) => box.x + box.width)) + PAD;
  const maxY = Math.max(source.y, target.y, ...obstacles.map((box) => box.y + box.height)) + PAD;
  const columns = Math.max(2, Math.ceil((maxX - minX) / CELL));
  const rows = Math.max(2, Math.ceil((maxY - minY) / CELL));
  const index = (column: number, row: number): number => row * columns + column;
  const point = (column: number, row: number): Point => ({
    x: minX + (column + 0.5) * CELL,
    y: minY + (row + 0.5) * CELL,
  });
  const startColumn = Math.min(columns - 1, Math.max(0, Math.floor((source.x - minX) / CELL)));
  const startRow = Math.min(rows - 1, Math.max(0, Math.floor((source.y - minY) / CELL)));
  const endColumn = Math.min(columns - 1, Math.max(0, Math.floor((target.x - minX) / CELL)));
  const endRow = Math.min(rows - 1, Math.max(0, Math.floor((target.y - minY) / CELL)));
  const blocked = new Uint8Array(columns * rows);
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
  return { columns, rows, start, end, blocked, point, index };
}

function findGridParents(grid: RouteGrid): Int32Array | null {
  const total = grid.columns * grid.rows;
  const parent = new Int32Array(total).fill(-1);
  const visited = new Uint8Array(total);
  const queue = new Int32Array(total);
  let head = 0;
  let tail = 0;
  queue[tail] = grid.start;
  tail += 1;
  visited[grid.start] = 1;
  const directions = [[1, 0], [-1, 0], [0, 1], [0, -1]];
  while (head < tail) {
    const current = queue[head];
    head += 1;
    if (current === grid.end) break;
    const column = current % grid.columns;
    const row = Math.floor(current / grid.columns);
    for (const [dx, dy] of directions) {
      const nextColumn = column + dx;
      const nextRow = row + dy;
      if (nextColumn < 0 || nextColumn >= grid.columns || nextRow < 0 || nextRow >= grid.rows) continue;
      const next = grid.index(nextColumn, nextRow);
      if (visited[next] || grid.blocked[next]) continue;
      visited[next] = 1;
      parent[next] = current;
      queue[tail] = next;
      tail += 1;
    }
  }
  return visited[grid.end] ? parent : null;
}

function traceGridCells(
  grid: RouteGrid,
  parent: Int32Array,
  source: Point,
  target: Point,
): Point[] {
  const cells: Point[] = [];
  let cursor = grid.end;
  while (cursor !== -1) {
    cells.push(grid.point(cursor % grid.columns, Math.floor(cursor / grid.columns)));
    if (cursor === grid.start) break;
    cursor = parent[cursor];
  }
  cells.reverse();
  // The grid center nearest a handle can sit slightly behind the handle. Use
  // the exact approach points for the first/last cell so the path leaves and
  // enters the card in the handle's direction instead of kinking backwards.
  if (cells.length > 0) {
    cells[0] = source;
    cells[cells.length - 1] = target;
  }
  return cells;
}

function routeAround(
  source: Point,
  target: Point,
  obstacles: EdgeObstacle[],
): Point[] | null {
  const grid = buildRouteGrid(source, target, obstacles);
  const parent = findGridParents(grid);
  if (!parent) return null;
  return simplify([source, ...traceGridCells(grid, parent, source, target), target]);
}

function pathFromPoints(points: Point[]): string {
  return points.map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x},${point.y}`).join(' ');
}

function pathClear(points: Point[], obstacles: EdgeObstacle[]): boolean {
  for (let index = 1; index < points.length; index += 1) {
    if (segmentBlocked(points[index - 1], points[index], obstacles)) return false;
  }
  return true;
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
    // The route starts and ends outside the handles. Keep the endpoint cards
    // out of pathfinding so a dense neighbour cannot trap the route inside the
    // source/target card.
    const obstacles = (data?.obstacles ?? []).filter((box) => box.id !== source && box.id !== target);
    const sourcePoint = { x: sourceX, y: sourceY };
    const targetPoint = { x: targetX, y: targetY };
    // Lift both markers just outside the cards. A marker whose tip sits exactly
    // on the handle can be covered by the opaque card and its shadow.
    const sourceEnd = approach(sourcePoint, sourcePosition, HANDLE_GAP);
    const targetEnd = approach(targetPoint, targetPosition, HANDLE_GAP);
    if (obstacles.length > 0) {
      const sourceApproach = clearApproach(sourcePoint, sourcePosition, APPROACH, obstacles);
      const targetApproach = clearApproach(targetPoint, targetPosition, APPROACH, obstacles);
      const candidates = [
        routeAround(sourceApproach, targetApproach, obstacles),
        visibilityRoute(sourceApproach, targetApproach, obstacles),
      ];
      for (const points of candidates) {
        if (!points) continue;
        const full = simplify([sourceEnd, ...points, targetEnd]);
        if (pathClear(full, obstacles)) {
          const middle = full[Math.floor(full.length / 2)];
          return { path: pathFromPoints(full), labelX: middle.x, labelY: middle.y };
        }
      }
    }
    const [path, labelX, labelY] = getSmoothStepPath({
      sourceX: sourceEnd.x,
      sourceY: sourceEnd.y,
      sourcePosition,
      targetX: targetEnd.x,
      targetY: targetEnd.y,
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
