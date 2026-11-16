import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { FlowNode } from './graph';

function fileBase(file: string): string {
  return file.split('/').at(-1) ?? file;
}

const GENERIC_MODULE_STEMS: Record<string, true> = {
  mod: true,
  index: true,
  __init__: true,
  init: true,
};

/** Top-level functions have no class; use their source module as the container label. */
function moduleLabel(file: string): string {
  const parts = file.split(/[\\/]/).filter(Boolean);
  const base = parts.pop() ?? file;
  const stem = base.replace(/\.[^.]+$/, '');
  if (GENERIC_MODULE_STEMS[stem.toLowerCase()]) {
    return parts.at(-1) ?? stem;
  }
  return stem;
}

export const NodeCard = memo(function NodeCard({ data, selected }: NodeProps<FlowNode>) {
  const { node, dimmed, highlighted, entryPoint, route } = data;
  const isSelected = selected || data.selected;
  const metrics = node.metrics;
  const dead = metrics.deadCodeCandidate && !entryPoint;
  const classes = [
    'node-card',
    `kind-${node.kind}`,
    isSelected ? 'selected' : '',
    highlighted ? 'highlighted' : '',
    dimmed ? 'dimmed' : '',
    dead ? 'dead' : '',
    node.external ? 'external' : '',
    entryPoint ? 'entry-root' : '',
    route ? 'route' : '',
  ].filter(Boolean).join(' ');
  const tooltip = [
    `${node.qname}  (${node.kind})`,
    `${node.file}:${node.line}`,
    `fan-in ${metrics.fanIn} · fan-out ${metrics.fanOut} · reach ${metrics.transitiveFanOut} · cyclomatic ${node.cyclomatic} · loc ${node.loc}`,
    dead ? 'dead-code candidate' : '',
    metrics.inCycle ? `in a cycle (SCC ${metrics.sccSize})` : '',
  ].filter(Boolean).join('\n');
  const split = node.qname.lastIndexOf('.');
  const container = split > 0 ? node.qname.slice(0, split) : '';
  const member = split > 0 ? node.qname.slice(split + 1) : node.qname;
  const module = moduleLabel(node.file);
  const label = container || (module.toLowerCase() === member.toLowerCase() ? '' : module);

  return (
    <div className={classes} title={tooltip}>
      <Handle id="target-top" type="target" position={Position.Top} className="rf-handle" isConnectable={false} />
      <Handle id="target-left" type="target" position={Position.Left} className="rf-handle" isConnectable={false} />
      <Handle id="target-right" type="target" position={Position.Right} className="rf-handle" isConnectable={false} />
      <Handle id="source-left" type="source" position={Position.Left} className="rf-handle" isConnectable={false} />
      <div className="card-top">
        <span className="card-dot" />
        <div className="card-title">
          {label ? <span className="card-class" title={container || node.file}>{label}</span> : null}
          <span className="card-name">{member}</span>
        </div>
        <div className="card-chips">
          {entryPoint ? (
            <span className="card-chip accent" title={`reaches ${metrics.transitiveFanOut} callables`}>root</span>
          ) : null}
          {metrics.inCycle ? <span className="card-chip cycle">cycle</span> : null}
          {dead ? <span className="card-chip danger">dead</span> : null}
          {node.isEntryLike ? <span className="card-chip success">entry</span> : null}
        </div>
      </div>
      <div className="card-meta">
        <span title="distinct callers">in <b>{metrics.fanIn}</b></span>
        <span title="distinct callees">out <b>{metrics.fanOut}</b></span>
        <span title="cyclomatic complexity">cc <b>{node.cyclomatic}</b></span>
        <span className="card-file" title={node.file}>{fileBase(node.file)}</span>
      </div>
      <Handle id="source-bottom" type="source" position={Position.Bottom} className="rf-handle" isConnectable={false} />
      <Handle id="source-right" type="source" position={Position.Right} className="rf-handle" isConnectable={false} />
    </div>
  );
});
