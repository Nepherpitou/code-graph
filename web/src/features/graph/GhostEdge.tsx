import { useId, memo } from 'react';
import { EdgeLabelRenderer, type Edge, type EdgeProps } from '@xyflow/react';

import { GHOST_STUB, type GhostEdgeData } from './graph';

/**
 * A short stub from the right edge of a card that fades into the canvas.
 * It marks connections that exist in the full graph but are absent from the
 * current slice, without drawing one line per hidden edge.
 */
export const GhostEdge = memo(function GhostEdge({
  sourceX,
  sourceY,
  data,
}: EdgeProps<Edge<GhostEdgeData>>) {
  const gradientId = useId().replace(/[^a-zA-Z0-9_-]/g, '');
  const hidden = data?.hidden ?? 0;
  const width = data?.nodeWidth ?? 0;
  const height = data?.nodeHeight ?? 0;
  // Cards have their source handle at the bottom center; start the stub at
  // the right-hand edge instead.
  const startX = sourceX + width / 2;
  const startY = sourceY - height / 2;
  const endX = startX + GHOST_STUB;
  const endY = startY;
  const labelX = startX + GHOST_STUB * 0.58;
  const labelY = startY;
  const path = `M ${startX},${startY} C ${startX + 16},${startY} ${endX - 16},${endY} ${endX},${endY}`;

  return (
    <>
      <defs>
        <linearGradient
          id={gradientId}
          gradientUnits="userSpaceOnUse"
          x1={startX}
          y1={startY}
          x2={endX}
          y2={endY}
        >
          <stop offset="0%" stopColor="var(--muted)" stopOpacity="0.7" />
          <stop offset="65%" stopColor="var(--muted)" stopOpacity="0.3" />
          <stop offset="100%" stopColor="var(--muted)" stopOpacity="0" />
        </linearGradient>
      </defs>
      <path
        d={path}
        fill="none"
        stroke={`url(#${gradientId})`}
        strokeWidth={1.4}
        strokeDasharray="4 4"
        strokeLinecap="round"
        pointerEvents="none"
      />
      {hidden > 0 ? (
        <EdgeLabelRenderer>
          <div
            className="ghost-edge-label"
            style={{ transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }}
            title={`${hidden} connection${hidden === 1 ? '' : 's'} outside this slice`}
          >
            +{hidden}
          </div>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
});
