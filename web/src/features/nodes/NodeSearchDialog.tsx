import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';

import type { GraphNode } from '../../types';
import { moduleLabel, splitNodeName } from '../../shared/format';

const ROW_HEIGHT = 118;
const OVERSCAN = 4;
const RESULT_LIMIT = 100;

interface Props {
  nodes: GraphNode[];
  onSelect: (node: GraphNode) => void;
  onClose: () => void;
}

function searchText(node: GraphNode): string {
  return [
    node.qname,
    node.file,
    node.kind,
    node.id,
    node.declaration,
  ].join('\n').toLowerCase();
}

function sortedNodes(nodes: GraphNode[]): GraphNode[] {
  return [...nodes].sort((left, right) =>
    left.qname.localeCompare(right.qname)
    || left.file.localeCompare(right.file)
    || left.id.localeCompare(right.id));
}

/** Same triage order as `audit top --order-by hotspot`. */
function auditTopNodes(nodes: GraphNode[]): GraphNode[] {
  return nodes
    .filter((node) => !node.external)
    .sort((left, right) =>
      right.metrics.hotspot - left.metrics.hotspot
      || left.qname.localeCompare(right.qname)
      || left.id.localeCompare(right.id))
    .slice(0, RESULT_LIMIT);
}

export function NodeSearchDialog({ nodes, onSelect, onClose }: Props) {
  const [query, setQuery] = useState('');
  const [scrollTop, setScrollTop] = useState(0);
  const [activeIndex, setActiveIndex] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const ordered = useMemo(() => sortedNodes(nodes), [nodes]);
  const auditTop = useMemo(() => auditTopNodes(nodes), [nodes]);
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const matches = useMemo(() => {
    if (terms.length === 0) return [];
    return ordered.filter((node) => {
      const haystack = searchText(node);
      return terms.every((term) => haystack.includes(term));
    });
  }, [ordered, terms]);
  const empty = terms.length === 0;
  const results = empty ? auditTop : matches.slice(0, RESULT_LIMIT);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const sync = (): void => setViewportHeight(element.clientHeight);
    sync();
    const observer = new ResizeObserver(sync);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    setActiveIndex(0);
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [query]);

  const active = results[activeIndex] ?? null;
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const end = Math.min(
    results.length,
    Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN,
  );
  const visible = results.slice(start, end);

  const moveActive = (nextIndex: number): void => {
    if (results.length === 0) return;
    const bounded = Math.max(0, Math.min(results.length - 1, nextIndex));
    setActiveIndex(bounded);
    const element = scrollRef.current;
    if (!element) return;
    const top = bounded * ROW_HEIGHT;
    const bottom = top + ROW_HEIGHT;
    if (top < element.scrollTop) element.scrollTop = top;
    else if (bottom > element.scrollTop + element.clientHeight) {
      element.scrollTop = bottom - element.clientHeight;
    }
  };

  const choose = (node: GraphNode | null): void => {
    if (node) onSelect(node);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveActive(activeIndex + 1);
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveActive(activeIndex - 1);
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      choose(active);
    }
  };

  return (
    <div className="node-search-overlay" onMouseDown={onClose}>
      <section
        className={`node-search-dialog${empty ? ' is-empty' : ''}`}
        role="dialog"
        aria-modal="true"
        aria-label="Search nodes"
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <header className="node-search-header">
          <div className="node-search-field">
            <svg className="node-search-icon" viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="11" cy="11" r="7" />
              <path d="m20 20-4.2-4.2" />
            </svg>
            <input
              ref={inputRef}
              className="node-search-input"
              type="search"
              value={query}
              placeholder="Search by class, member, file, kind, or node id…"
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => setQuery(event.target.value)}
              aria-label="Search nodes"
            />
            {query ? (
              <button
                className="node-search-clear"
                type="button"
                title="Clear search"
                aria-label="Clear search"
                onClick={() => setQuery('')}
              >
                ×
              </button>
            ) : null}
          </div>
          <button
            className="node-search-close"
            type="button"
            onClick={onClose}
            title="Close search (Esc)"
            aria-label="Close search"
          >
            Close
          </button>
        </header>

        <div className="node-search-summary">
          <span className="node-search-count">
            {empty
              ? `${results.length} top nodes by audit hotspot`
              : `${results.length}${matches.length > results.length ? ` of ${matches.length}` : ''} matching nodes`}
          </span>
          <span className="node-search-hint">
            <kbd>↑</kbd><kbd>↓</kbd> move · <kbd>Enter</kbd> open · <kbd>Esc</kbd> close
          </span>
        </div>

        <div
          className="node-search-results"
          ref={scrollRef}
          onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        >
          {results.length === 0 ? (
            <div className="node-search-empty">No matching nodes.</div>
          ) : (
            <div className="node-search-track" style={{ height: results.length * ROW_HEIGHT }}>
              {visible.map((node, offset) => {
                const index = start + offset;
                const { container, member } = splitNodeName(node.qname);
                const className = container || moduleLabel(node.file);
                const activeRow = index === activeIndex;
                return (
                  <button
                    key={node.id}
                    type="button"
                    className={`node-search-row kind-${node.kind}${activeRow ? ' active' : ''}`}
                    style={{ top: index * ROW_HEIGHT }}
                    onClick={() => choose(node)}
                    onMouseEnter={() => setActiveIndex(index)}
                  >
                    <span className="node-search-body">
                      <span className="node-search-head">
                        <span className="node-search-kind">{node.kind}</span>
                        <span className="node-search-class">{className}</span>
                        <span className="node-search-member">{member}</span>
                        <span className="node-search-chips">
                          {node.isEntryLike ? <span className="card-chip success">entry</span> : null}
                          {node.metrics.inCycle ? <span className="card-chip cycle">cycle</span> : null}
                          {node.metrics.deadCodeCandidate ? <span className="card-chip danger">dead</span> : null}
                          {node.synthetic ? <span className="card-chip hidden">type</span> : null}
                        </span>
                      </span>
                      <span className="node-search-file">{node.file}:{node.line}</span>
                      <span className="node-search-metrics">
                        <span>fan-in <b>{node.metrics.fanIn}</b></span>
                        <span>fan-out <b>{node.metrics.fanOut}</b></span>
                        <span>reach <b>{node.metrics.transitiveFanOut}</b></span>
                        <span>depth <b>{node.metrics.depth}</b></span>
                        <span>height <b>{node.metrics.height}</b></span>
                        <span>cc <b>{node.cyclomatic}</b></span>
                        <span>loc <b>{node.loc}</b></span>
                        <span>hotspot <b>{node.metrics.hotspot.toFixed(2)}</b></span>
                      </span>
                      <code className="node-search-id" title={node.id}>{node.id}</code>
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
