import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { fileBase, moduleLabel, splitNodeName } from '../../shared/format';
import type { FlowNode } from './graph';

export const NodeCard = memo(function NodeCard({ data, selected }: NodeProps<FlowNode>) {
  const { node, dimmed, highlighted, entryPoint, route, hidden } = data;
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
  const hiddenTitle = hidden
    ? `${hidden.hidden} neighbour${hidden.hidden === 1 ? '' : 's'} outside this slice\n`
      + `${hidden.hiddenIn} incoming · ${hidden.hiddenOut} outgoing`
    : '';
  const tooltip = [
    `${node.qname}  (${node.kind})`,
    `${node.file}:${node.line}`,
    `fan-in ${metrics.fanIn} · fan-out ${metrics.fanOut} · reach ${metrics.transitiveFanOut} · cyclomatic ${node.cyclomatic} · loc ${node.loc}`,
    dead ? 'dead-code candidate' : '',
    metrics.inCycle ? `in a cycle (SCC ${metrics.sccSize})` : '',
    hiddenTitle,
  ].filter(Boolean).join('\n');
  const { container, member } = splitNodeName(node.qname);
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
          {hidden ? (
            <span className="card-chip hidden" title={hiddenTitle}>
              +{hidden.hidden}
            </span>
          ) : null}
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
