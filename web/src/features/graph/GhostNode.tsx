import { memo } from 'react';
import { Handle, Position } from '@xyflow/react';

/** Invisible target for a ghost edge; it only anchors the fade-out stub. */
export const GhostNode = memo(function GhostNode() {
  return (
    <div className="ghost-node" aria-hidden>
      <Handle type="target" position={Position.Left} className="rf-handle" isConnectable={false} />
    </div>
  );
});
