import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';

import type { GraphNode } from '../../types';
import { moduleLabel, splitNodeName } from '../../shared/format';

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

function escapePattern(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function highlight(text: string, terms: string[]): ReactNode {
  if (terms.length === 0) return text;
  const pattern = new RegExp(`(${terms.map(escapePattern).join('|')})`, 'ig');
  return text.split(pattern).map((part, index) =>
    terms.some((term) => part.toLowerCase() === term)
      ? <mark key={index}>{part}</mark>
      : part);
}

/** Prefer exact/prefix symbol hits, then audit rank; alphabetical is the tie-break. */
function matchScore(node: GraphNode, query: string): number {
  const qname = node.qname.toLowerCase();
  const name = node.name.toLowerCase();
  const file = node.file.toLowerCase();
  let score = node.metrics.hotspot;
  if (qname === query) score += 1200;
  else if (name === query) score += 900;
  else if (name.startsWith(query)) score += 650;
  else if (qname.endsWith(`.${query}`)) score += 550;
  else if (qname.includes(query)) score += 300;
  if (file.includes(query)) score += 40;
  return score;
}

export function NodeSearchDialog({ nodes, onSelect, onClose }: Props) {
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const rowsRef = useRef<Array<HTMLButtonElement | null>>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  const ordered = useMemo(() => sortedNodes(nodes), [nodes]);
  const auditTop = useMemo(() => auditTopNodes(nodes), [nodes]);
  const normalized = query.trim().toLowerCase();
  const terms = useMemo(
    () => normalized.split(/\s+/).filter(Boolean),
    [normalized],
  );
  const matches = useMemo(() => {
    if (terms.length === 0) return [];
    return ordered
      .filter((node) => terms.every((term) => searchText(node).includes(term)))
      .map((node) => ({ node, score: matchScore(node, normalized) }))
      .sort((left, right) =>
        right.score - left.score
        || left.node.qname.localeCompare(right.node.qname)
        || left.node.id.localeCompare(right.node.id))
      .map(({ node }) => node);
  }, [normalized, ordered, terms]);
  const empty = terms.length === 0;
  const results = empty ? auditTop : matches.slice(0, RESULT_LIMIT);
  const active = results[activeIndex] ?? null;

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  useEffect(() => {
    rowsRef.current[activeIndex]?.scrollIntoView({ block: 'nearest' });
  }, [activeIndex]);

  const choose = (node: GraphNode | null): void => {
    if (node) onSelect(node);
  };

  const moveActive = (nextIndex: number): void => {
    if (results.length === 0) return;
    setActiveIndex(Math.max(0, Math.min(results.length - 1, nextIndex)));
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (event.key === 'Escape') {
      event.preventDefault();
      if (query) {
        setQuery('');
        return;
      }
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
        className="node-search-dialog"
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
              placeholder="Search nodes by name, file, kind, or id…"
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => setQuery(event.target.value)}
              aria-label="Search nodes"
              aria-controls="node-search-results"
              aria-activedescendant={active ? `node-search-row-${activeIndex}` : undefined}
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
        </header>

        <div className="node-search-results" id="node-search-results" role="listbox">
          {results.length === 0 ? (
            <div className="node-search-empty">
              <strong>No matching nodes.</strong>
              <span>Try a shorter query, a file name, or a node id.</span>
              <button className="button" type="button" onClick={() => setQuery('')}>
                Clear search
              </button>
            </div>
          ) : results.map((node, index) => {
            const { container, member } = splitNodeName(node.qname);
            const className = container || moduleLabel(node.file);
            const activeRow = index === activeIndex;
            return (
              <button
                key={node.id}
                id={`node-search-row-${index}`}
                ref={(element) => { rowsRef.current[index] = element; }}
                type="button"
                role="option"
                aria-selected={activeRow}
                tabIndex={-1}
                className={`node-search-row kind-${node.kind}${activeRow ? ' active' : ''}`}
                onClick={() => choose(node)}
                onMouseEnter={() => setActiveIndex(index)}
              >
                <span className="node-search-kind">{node.kind}</span>
                <span className="node-search-main">
                  <span className="node-search-title">
                    <span className="node-search-class">{highlight(className, terms)}</span>
                    <span className="node-search-member">{highlight(member, terms)}</span>
                  </span>
                  <span className="node-search-sub">
                    <span className="node-search-file" title={`${node.file}:${node.line}`}>{node.file}:{node.line}</span>
                    <span className="node-search-metrics">
                      in <b>{node.metrics.fanIn}</b> · out <b>{node.metrics.fanOut}</b>
                      {' · '}hotspot <b>{node.metrics.hotspot.toFixed(2)}</b>
                    </span>
                  </span>
                </span>
                <span className="node-search-chips">
                  {node.isEntryLike ? <span className="card-chip success">entry</span> : null}
                  {node.metrics.inCycle ? <span className="card-chip cycle">cycle</span> : null}
                  {node.metrics.deadCodeCandidate ? <span className="card-chip danger">dead</span> : null}
                  {node.synthetic ? <span className="card-chip hidden">type</span> : null}
                </span>
              </button>
            );
          })}
        </div>

        <footer className="node-search-footer">
          <span className="node-search-context">
            {empty
              ? `Top ${results.length} nodes by audit hotspot`
              : `${results.length}${matches.length > results.length ? ` of ${matches.length}` : ''} matches for “${query.trim()}”`}
            {' · searching name, file, kind, and id'}
          </span>
          <span className="node-search-hint">
            <kbd>↑</kbd><kbd>↓</kbd> navigate · <kbd>Enter</kbd> open · <kbd>Esc</kbd> {query ? 'clear' : 'close'}
          </span>
        </footer>
      </section>
    </div>
  );
}
