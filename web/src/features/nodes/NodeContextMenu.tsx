import { useEffect, useRef } from 'react';

import type { GraphNode } from '../../types';

export interface ContextAction {
  label: string;
  onSelect: () => void;
}

interface Props {
  node: GraphNode;
  x: number;
  y: number;
  actions: ContextAction[];
  onClose: () => void;
}

export function NodeContextMenu({ node, x, y, actions, onClose }: Props) {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent): void => {
      if (!menuRef.current?.contains(event.target as globalThis.Node)) onClose();
    };
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [onClose]);

  const left = Math.min(x, Math.max(8, window.innerWidth - 232));
  const top = Math.min(y, Math.max(8, window.innerHeight - actions.length * 30 - 48));

  return (
    <div ref={menuRef} className="node-context-menu" style={{ left, top }} role="menu">
      <div className="node-context-title" title={node.id}>
        {node.qname}
      </div>
      {actions.map((action) => (
        <button
          key={action.label}
          className="node-context-action"
          role="menuitem"
          onClick={() => {
            action.onSelect();
            onClose();
          }}
        >
          {action.label}
        </button>
      ))}
    </div>
  );
}
