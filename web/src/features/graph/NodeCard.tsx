import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { FlowNode } from './graph';

function fileBase(file: string): string {
  return file.split('/').at(-1) ?? file;
}

export function NodeCard({ data, selected }: NodeProps<FlowNode>) {
  const { node, dimmed, highlighted, entryPoint } = data;
  const metrics = node.metrics;
  const dead = metrics.deadCodeCandidate && !entryPoint;
  const classes = [
    'node-card',
    `kind-${node.kind}`,
    selected ? 'selected' : '',
    highlighted ? 'highlighted' : '',
    dimmed ? 'dimmed' : '',
    dead ? 'dead' : '',
    node.external ? 'external' : '',
    entryPoint ? 'entry-root' : '',
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

  return (
    <div className={classes} title={tooltip}>
      <Handle type="target" position={Position.Top} className="rf-handle" isConnectable={false} />
      <div className="card-top">
        <span className="card-dot" />
        <div className="card-title">
          {container ? <span className="card-class" title={container}>{container}</span> : null}
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
      <Handle type="source" position={Position.Bottom} className="rf-handle" isConnectable={false} />
    </div>
  );
}
