/* Convergence cut: eight focused directions for the code-graph web UI.
   Shared shell, graph renderer, and mode-specific surfaces. */
(() => {
  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
  const esc = (value) => String(value ?? '').replace(/[&<>"']/g, (char) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[char]));

  function toast(message) {
    const node = document.createElement('div');
    node.className = 'cx-toast';
    node.textContent = message;
    document.body.appendChild(node);
    setTimeout(() => node.remove(), 1800);
  }

  const PAGES = {
    focus: 'c01-focus-workbench.html',
    path: 'c02-path-mode.html',
    impact: 'c03-impact-mode.html',
    compare: 'c04-compare-pins.html',
    table: 'c05-table-drawer.html',
    palette: 'c06-command-resume.html',
    overview: 'c07-overview-inspector.html',
    minimal: 'c08-minimal-canvas.html',
  };

  const META = {
    focus: { number: 'C01', title: 'Focus workbench', kicker: 'Shell · focus', note: 'Graph first, inspector on selection, navigator reduced to search and recent work.' },
    path: { number: 'C02', title: 'Path mode', kicker: 'Mode · path', note: 'Entry → exit is a strip on the canvas, not a separate screen.' },
    impact: { number: 'C03', title: 'Impact mode', kicker: 'Mode · impact', note: 'Reverse callers and affected surface on demand, without a triage queue.' },
    compare: { number: 'C04', title: 'Compare pins', kicker: 'Mode · compare', note: 'Two pinned symbols, one graph, a diff inspector.' },
    table: { number: 'C05', title: 'Linked table', kicker: 'Surface · table', note: 'The table is a drawer that brushes the graph, not the primary surface.' },
    palette: { number: 'C06', title: 'Command / resume', kicker: 'Surface · command', note: 'Search, recent snapshots, and actions; no home screen.' },
    overview: { number: 'C07', title: 'Overview inspector', kicker: 'Surface · overview', note: 'Overview is a drawer beside the graph, not a landing page.' },
    minimal: { number: 'C08', title: 'Minimal canvas', kicker: 'Density · minimal', note: 'Zero permanent chrome; selection opens a compact inspector.' },
  };

  const MODES = [
    ['focus', 'Focus'],
    ['path', 'Path'],
    ['impact', 'Impact'],
    ['compare', 'Compare'],
  ];

  const nameOf = (id) => CG.byId[id]?.name || id;
  const qnameOf = (id) => `${CG.byId[id]?.cls || ''}.${CG.byId[id]?.name || id}`;
  const activeMode = (variant) => (MODES.some(([id]) => id === variant) ? variant : 'focus');

  function topbar(variant) {
    const meta = META[variant];
    const active = activeMode(variant);
    const paletteControl = variant === 'palette'
      ? '<button class="cx-command" data-palette-open><span class="cx-kbd">⌘K</span><span>Search or run</span></button>'
      : `<a class="cx-command" href="${PAGES.palette}"><span class="cx-kbd">⌘K</span><span>Search or run</span></a>`;
    return `
      <header class="cx-top">
        <a class="cx-brand" href="index.html"><span class="cx-logo">◉</span><span>code-graph</span></a>
        <span class="cx-surface">${esc(meta.kicker)}</span>
        <div class="cx-crumbs">
          <span>checkout-service</span><i>/</i><b>src/domain</b>
          <span class="cx-snapshot">snapshot a3f10c2</span>
        </div>
        <nav class="cx-modes" aria-label="Work modes">
          ${MODES.map(([id, label]) => `
            <a class="cx-mode ${active === id ? 'active' : ''}" href="${PAGES[id]}">
              <span class="cx-dot ${id}"></span>${label}
            </a>`).join('')}
        </nav>
        <div class="cx-actions">
          ${paletteControl}
          <span class="cx-local">local · 6m</span>
        </div>
      </header>`;
  }

  function nav(variant) {
    const scopes = [
      ['checkout-service', '148 files', ''],
      ['src/http', '18', ''],
      ['src/domain', '42', 'active'],
      ['src/payments', '24', ''],
      ['src/infra', '31', ''],
    ];
    const entries = [
      ['checkout', 'CheckoutController.checkout', '1.8k'],
      ['handleEvent', 'PaymentWebhook.handleEvent', '722'],
      ['refund', 'RefundService.refund', '611'],
      ['createOrder', 'OrderService.createOrder', '488'],
    ];
    const recents = [
      ['a3f10c2', '6 min ago', 'current'],
      ['d91e2b4', '1 h ago', ''],
      ['main · local', '2 h ago', ''],
    ];
    return `
      <aside class="cx-nav">
        <div class="cx-nav-head"><span>Navigator</span><span class="cx-kbd">⌘K</span></div>
        <label class="cx-search">
          <span class="cx-search-icon">⌕</span>
          <input id="cx-nav-search" placeholder="Search functions, files…" autocomplete="off">
        </label>
        <div class="cx-nav-section">
          <div class="cx-nav-label">Scope</div>
          ${scopes.map(([label, count, state]) => `
            <button class="cx-nav-row ${state}" data-scope="${esc(label)}">
              <span class="cx-mono">${esc(label)}</span><span class="cx-nav-count">${esc(count)}</span>
            </button>`).join('')}
        </div>
        <div class="cx-nav-section">
          <div class="cx-nav-label">Entry points</div>
          <div id="cx-entry-list">
            ${entries.map(([id, label, reach]) => `
              <button class="cx-nav-row" data-node="${esc(id)}">
                <span class="cx-mono">${esc(label)}</span><span class="cx-nav-count">${esc(reach)}</span>
              </button>`).join('')}
          </div>
        </div>
        <div class="cx-nav-section">
          <div class="cx-nav-label">Recent</div>
          ${recents.map(([label, age, state]) => `
            <button class="cx-nav-row ${state}" data-recent="${esc(label)}">
              <span class="cx-mono">${esc(label)}</span><span class="cx-nav-count">${esc(age)}</span>
            </button>`).join('')}
        </div>
        <div class="cx-nav-foot"><span class="cx-status-dot"></span>local JSON · no API</div>
      </aside>`;
  }

  function metricFacts() {
    return `
      <div class="cx-facts">
        <div><b data-fanin>3</b><span>fan-in</span></div>
        <div><b data-fanout>4</b><span>fan-out</span></div>
        <div><b data-hotspot>88</b><span>hotspot</span></div>
        <div><b>831</b><span>reach</span></div>
      </div>`;
  }

  function relationsBlock() {
    return `
      <div class="cx-section">
        <div class="cx-label">Relations</div>
        <div id="cx-relations">${relationsHtml('createOrder')}</div>
      </div>`;
  }

  function relationsHtml(id) {
    const callers = (CG.callers[id] || []).slice(0, 4).map((source) => `
      <button class="cx-rel" data-node="${esc(source)}"><span class="cx-arrow">←</span><span class="cx-mono">${esc(nameOf(source))}</span><span class="cx-rel-meta">caller</span></button>`).join('');
    const callees = (CG.callees[id] || []).slice(0, 4).map((target) => `
      <button class="cx-rel" data-node="${esc(target)}"><span class="cx-arrow">→</span><span class="cx-mono">${esc(nameOf(target))}</span><span class="cx-rel-meta">callee</span></button>`).join('');
    return `${callers}${callees}` || '<div class="cx-empty">No direct relations.</div>';
  }

  function focusInspector(label = 'Focus') {
    return `
      <aside class="cx-inspect">
        <div class="cx-inspect-head">
          <div class="cx-label">${esc(label)}</div>
          <h2 data-selected-qname>${esc(qnameOf('createOrder'))}</h2>
          <div class="cx-file" data-selected-file>src/domain/OrderService.ts</div>
        </div>
        ${metricFacts()}
        ${relationsBlock()}
        <div class="cx-section">
          <div class="cx-label">Follow</div>
          <div class="cx-action-list">
            <a href="${PAGES.path}">Entry → exit path <span>→</span></a>
            <a href="${PAGES.impact}">Reverse impact <span>←</span></a>
            <a href="${PAGES.compare}">Pin and compare <span>⇄</span></a>
          </div>
        </div>
        <div class="cx-inspect-foot"><span class="cx-status-dot"></span>snapshot a3f10c2 · local · 6 min ago</div>
      </aside>`;
  }

  function impactInspector() {
    return `
      <aside class="cx-inspect">
        <div class="cx-inspect-head">
          <div class="cx-label">Impact · reverse</div>
          <h2 data-selected-qname>${esc(qnameOf('createOrder'))}</h2>
          <div class="cx-file" data-selected-file>src/domain/OrderService.ts</div>
        </div>
        <div class="cx-risk">
          <div class="cx-risk-value">0.62</div>
          <div><b>risk</b><span>3 direct callers · 5 transitive</span></div>
        </div>
        <div class="cx-risk-meter"><i style="width:62%"></i></div>
        <div class="cx-section">
          <div class="cx-label">Affected surface</div>
          ${['checkout', 'handleEvent', 'refund', 'legacyRefund'].map((id) => `
            <button class="cx-rel" data-node="${esc(id)}">
              <span class="cx-arrow">←</span><span class="cx-mono">${esc(nameOf(id))}</span>
              <span class="cx-rel-meta">${id === 'checkout' ? 'entry' : 'caller'}</span>
            </button>`).join('')}
        </div>
        <div class="cx-section">
          <div class="cx-label">Evidence</div>
          <div class="cx-evidence"><b>No direct tests</b><span>coverage inferred from call edges only</span></div>
          <div class="cx-evidence"><b>In cycle: no</b><span>shortest entry distance 2</span></div>
        </div>
        <div class="cx-inspect-foot"><a href="${PAGES.table}">Open affected table ↗</a></div>
      </aside>`;
  }

  function compareInspector() {
    return `
      <aside class="cx-inspect">
        <div class="cx-compare-head">
          <div class="cx-label">Compare · pinned</div>
          <button class="cx-icon" data-swap title="Swap pins">⇄</button>
        </div>
        <div class="cx-pin-card">
          <span class="cx-pin-mark">A</span>
          <h2 data-pin="a">${esc(qnameOf('createOrder'))}</h2>
          <div class="cx-file">src/domain/OrderService.ts</div>
          <div class="cx-mini-facts"><span>in 3</span><span>out 4</span><span>hotspot 88</span></div>
        </div>
        <div class="cx-pin-card">
          <span class="cx-pin-mark">B</span>
          <h2 data-pin="b">${esc(qnameOf('validateCart'))}</h2>
          <div class="cx-file">src/http/CheckoutController.ts</div>
          <div class="cx-mini-facts"><span>in 3</span><span>out 2</span><span>hotspot 38</span></div>
        </div>
        <div class="cx-section">
          <div class="cx-label">Diff</div>
          <div class="cx-diff"><span>Shared callers</span><b>checkout</b></div>
          <div class="cx-diff"><span>Only A</span><b>reserveInventory, authorize, finalizeOrder</b></div>
          <div class="cx-diff"><span>Only B</span><b>priceCart</b></div>
        </div>
        <div class="cx-section">
          <div class="cx-label">Shared context</div>
          <div class="cx-rel"><span class="cx-arrow">←</span><span class="cx-mono">checkout</span><span class="cx-rel-meta">entry</span></div>
          <div class="cx-rel"><span class="cx-arrow">→</span><span class="cx-mono">priceCart</span><span class="cx-rel-meta">both reach</span></div>
        </div>
        <div class="cx-inspect-foot"><a href="${PAGES.focus}">Back to focus ↗</a></div>
      </aside>`;
  }

  function overviewInspector() {
    return `
      <aside class="cx-inspect cx-overview-inspect">
        <div class="cx-inspect-head">
          <div class="cx-label">Overview · snapshot</div>
          <h2>checkout-service</h2>
          <div class="cx-file">148 files · 2,462 symbols · 5,840 edges</div>
        </div>
        <div class="cx-facts">
          <div><b>12</b><span>signals</span></div>
          <div><b>3</b><span>cycles</span></div>
          <div><b>7</b><span>dead</span></div>
          <div><b>8.4s</b><span>analysis</span></div>
        </div>
        <div class="cx-section">
          <div class="cx-label">Top hotspots</div>
          ${[...CG.nodes].sort((a, b) => b.hotspot - a.hotspot).slice(0, 6).map((node) => `
            <button class="cx-rel" data-node="${esc(node.id)}">
              <span class="cx-mono">${esc(node.cls)}.${esc(node.name)}</span>
              <span class="cx-rel-meta">${node.hotspot}</span>
            </button>`).join('')}
        </div>
        <div class="cx-section">
          <div class="cx-label">Modules</div>
          ${CG.modules.map((module) => `
            <div class="cx-module"><span>${esc(module.label)}</span><b>${CG.nodes.filter((node) => node.x >= module.x && node.x < module.x + module.w).length} nodes</b></div>`).join('')}
        </div>
        <div class="cx-inspect-foot"><a href="${PAGES.focus}">Return to focus ↗</a></div>
      </aside>`;
  }

  function modebar(variant) {
    if (variant === 'path') {
      return `
        <div class="cx-modebar cx-pathbar">
          <span class="cx-label">Path</span>
          <div class="cx-path-strip">
            ${CG.checkoutPath.map((id, index) => `
              <button class="cx-path-step ${id === 'createOrder' ? 'active' : ''}" data-path-index="${index}">
                <span>${index + 1}</span>${esc(nameOf(id))}
              </button>${index < CG.checkoutPath.length - 1 ? '<i class="cx-sep">→</i>' : ''}`).join('')}
          </div>
          <button class="cx-icon" data-play title="Play path">▶</button>
          <button class="cx-icon" data-graph-action="fit" title="Fit graph">⌖</button>
        </div>`;
    }
    if (variant === 'impact') {
      return `
        <div class="cx-modebar">
          <span class="cx-label">Impact</span>
          <b class="cx-mono">createOrder</b>
          <span class="cx-pill">3 direct</span><span class="cx-pill">5 transitive</span>
          <span class="cx-pill danger">risk 0.62</span>
          <button class="cx-toggle" data-impact-toggle>tests: unknown</button>
          <span class="cx-spacer"></span>
          <button class="cx-icon" data-graph-action="fit" title="Fit graph">⌖</button>
        </div>`;
    }
    if (variant === 'compare') {
      return `
        <div class="cx-modebar">
          <span class="cx-label">Compare</span>
          <span class="cx-pin-chip">A · createOrder</span><span class="cx-sep">vs</span>
          <span class="cx-pin-chip">B · validateCart</span>
          <button class="cx-toggle" data-swap>⇄ swap</button>
          <span class="cx-spacer"></span>
          <button class="cx-icon" data-graph-action="fit" title="Fit graph">⌖</button>
        </div>`;
    }
    if (variant === 'table') {
      return `
        <div class="cx-modebar">
          <span class="cx-label">Linked table</span>
          <label class="cx-inline-search"><span>⌕</span><input id="cx-table-search" placeholder="Filter nodes…" autocomplete="off"></label>
          <span class="cx-hint">hover a row to brush the graph</span>
          <span class="cx-spacer"></span>
          <button class="cx-toggle" data-drawer-toggle>hide table</button>
          <button class="cx-icon" data-graph-action="fit" title="Fit graph">⌖</button>
        </div>`;
    }
    if (variant === 'overview') {
      return `
        <div class="cx-modebar">
          <span class="cx-label">Overview</span>
          <span class="cx-pill">148 files</span><span class="cx-pill">2,462 symbols</span>
          <span class="cx-pill">12 signals</span><span class="cx-pill danger">3 cycles</span>
          <span class="cx-spacer"></span>
          <button class="cx-icon" data-graph-action="fit" title="Fit graph">⌖</button>
        </div>`;
    }
    if (variant === 'minimal') {
      return `
        <div class="cx-modebar cx-min-bar">
          <span class="cx-label">src/domain</span>
          <span class="cx-hint">click a node · selection opens inspector · ⌘K</span>
          <span class="cx-spacer"></span>
          <span class="cx-mono cx-faint">20 nodes · 23 edges</span>
        </div>`;
    }
    if (variant === 'palette') {
      return `
        <div class="cx-modebar">
          <span class="cx-label">Command surface</span>
          <span class="cx-hint">⌘K opens search and actions; no home screen</span>
          <span class="cx-spacer"></span>
          <button class="cx-icon" data-graph-action="fit" title="Fit graph">⌖</button>
        </div>`;
    }
    return `
      <div class="cx-modebar">
        <span class="cx-label">Focus</span>
        <span class="cx-hint">click a node to inspect · path and impact are modes, not screens</span>
        <span class="cx-spacer"></span>
        <button class="cx-icon" data-graph-action="fit" title="Fit graph">⌖</button>
      </div>`;
  }

  function drawer(variant) {
    if (variant === 'table') {
      const rows = [...CG.nodes].sort((a, b) => b.hotspot - a.hotspot).slice(0, 12);
      return `
        <div class="cx-drawer open" id="cx-drawer">
          <div class="cx-drawer-head">
            <div><span class="cx-label">All nodes</span><span class="cx-hint">12 of 20 · sorted by hotspot</span></div>
            <div class="cx-drawer-actions"><button class="cx-toggle" data-drawer-toggle>collapse</button></div>
          </div>
          <div class="cx-table-scroll">
            <table class="cx-table">
              <thead><tr><th>Symbol</th><th>Kind</th><th>In</th><th>Out</th><th>Hotspot</th></tr></thead>
              <tbody>
                ${rows.map((node) => `
                  <tr data-node="${esc(node.id)}" tabindex="0">
                    <td><b>${esc(node.cls)}.${esc(node.name)}</b><span>${esc(node.file)}</span></td>
                    <td class="cx-mono">${esc(node.kind)}</td>
                    <td class="cx-mono">${node.fanIn}</td>
                    <td class="cx-mono">${node.fanOut}</td>
                    <td class="cx-mono">${node.hotspot}</td>
                  </tr>`).join('')}
              </tbody>
            </table>
          </div>
        </div>`;
    }
    if (variant === 'path') {
      return `
        <div class="cx-drawer open" id="cx-drawer">
          <div class="cx-drawer-head">
            <div><span class="cx-label">Path ledger</span><span class="cx-hint">${CG.checkoutPath.length} steps · checkout → capture</span></div>
            <div class="cx-drawer-actions"><a class="cx-toggle" href="${PAGES.table}">table view ↗</a></div>
          </div>
          <div class="cx-table-scroll">
            <table class="cx-table">
              <thead><tr><th>Step</th><th>Symbol</th><th>File</th><th>Calls</th></tr></thead>
              <tbody>
                ${CG.checkoutPath.map((id, index) => `
                  <tr data-node="${esc(id)}" data-path-index="${index}" class="${id === 'createOrder' ? 'selected' : ''}">
                    <td class="cx-mono">${index + 1}</td>
                    <td><b>${esc(nameOf(id))}</b><span>${esc(qnameOf(id))}</span></td>
                    <td class="cx-mono">${esc(CG.byId[id]?.file || '')}</td>
                    <td class="cx-mono">${(CG.callees[id] || []).length}</td>
                  </tr>`).join('')}
              </tbody>
            </table>
          </div>
        </div>`;
    }
    if (variant === 'impact') {
      return `
        <div class="cx-drawer open" id="cx-drawer">
          <div class="cx-drawer-head">
            <div><span class="cx-label">Affected table</span><span class="cx-hint">reverse callers of createOrder</span></div>
            <div class="cx-drawer-actions"><a class="cx-toggle" href="${PAGES.table}">all nodes ↗</a></div>
          </div>
          <div class="cx-table-scroll">
            <table class="cx-table">
              <thead><tr><th>Caller</th><th>Distance</th><th>Kind</th><th>Risk</th></tr></thead>
              <tbody>
                ${['checkout', 'handleEvent', 'refund', 'legacyRefund'].map((id, index) => `
                  <tr data-node="${esc(id)}">
                    <td><b>${esc(nameOf(id))}</b><span>${esc(qnameOf(id))}</span></td>
                    <td class="cx-mono">${index + 1}</td>
                    <td class="cx-mono">${esc(CG.byId[id]?.kind || '')}</td>
                    <td class="cx-mono ${index < 2 ? 'danger' : ''}">${index < 2 ? 'high' : 'medium'}</td>
                  </tr>`).join('')}
              </tbody>
            </table>
          </div>
        </div>`;
    }
    if (variant === 'minimal') return '';
    return `
      <div class="cx-status">
        <span><b>20</b> nodes · <b>23</b> edges</span>
        <span>snapshot a3f10c2</span>
        <span class="cx-spacer"></span>
        <span>F focus · P path · I impact · C compare</span>
        <span class="cx-status-ready">local</span>
      </div>`;
  }

  function palette() {
    return `
      <div class="cx-palette" id="cx-palette" hidden>
        <div class="cx-palette-box" role="dialog" aria-modal="true" aria-label="Command palette">
          <label class="cx-palette-search">
            <span>⌘</span><input id="cx-palette-input" placeholder="Search functions, snapshots, or actions…" autocomplete="off">
            <span class="cx-kbd">esc</span>
          </label>
          <div class="cx-palette-list" id="cx-palette-list">
            <div class="cx-palette-group"><span>Recent</span>
              <button class="cx-palette-item" data-command="resume"><b>Resume snapshot a3f10c2</b><small>6 min ago · local</small></button>
              <button class="cx-palette-item" data-command="json"><b>Open local graph.json</b><small>file</small></button>
            </div>
            <div class="cx-palette-group"><span>Modes</span>
              <a class="cx-palette-item" href="${PAGES.focus}"><b>Focus workbench</b><small>F</small></a>
              <a class="cx-palette-item" href="${PAGES.path}"><b>Path mode</b><small>P</small></a>
              <a class="cx-palette-item" href="${PAGES.impact}"><b>Impact mode</b><small>I</small></a>
              <a class="cx-palette-item" href="${PAGES.compare}"><b>Compare pins</b><small>C</small></a>
            </div>
            <div class="cx-palette-group"><span>Actions</span>
              <button class="cx-palette-item" data-command="focus"><b>Focus createOrder</b><small>selection</small></button>
              <button class="cx-palette-item" data-command="impact"><b>Show impact for createOrder</b><small>reverse</small></button>
              <button class="cx-palette-item" data-command="table"><b>Toggle linked table</b><small>drawer</small></button>
              <button class="cx-palette-item" data-command="export"><b>Export PNG</b><small>snapshot</small></button>
            </div>
          </div>
          <div class="cx-palette-foot">↑↓ move · enter run · esc close</div>
        </div>
      </div>`;
  }

  function minimalInspector() {
    return `
      <aside class="cx-min-inspect" id="cx-min-inspect" hidden>
        <button class="cx-min-close" data-min-close aria-label="Close inspector">×</button>
        <div class="cx-label">Selected</div>
        <h2 data-selected-qname>${esc(qnameOf('createOrder'))}</h2>
        <div class="cx-file" data-selected-file>src/domain/OrderService.ts</div>
        <div class="cx-mini-facts"><span data-fanin>3</span><span data-fanout>4</span><span data-hotspot>88</span></div>
        <div class="cx-section"><div class="cx-label">Relations</div><div id="cx-relations">${relationsHtml('createOrder')}</div></div>
        <div class="cx-action-list">
          <a href="${PAGES.path}">Path <span>→</span></a>
          <a href="${PAGES.impact}">Impact <span>←</span></a>
        </div>
      </aside>`;
  }

  function shell(variant) {
    return `
      <div class="cx-app v-${variant}">
        ${topbar(variant)}
        <div class="cx-body">
          ${variant === 'minimal' ? '' : nav(variant)}
          <main class="cx-main">
            ${modebar(variant)}
            <div class="cx-canvas-wrap">
              <div class="cx-canvas graph-shell" id="cx-graph">
                <div class="cx-graph-hud">
                  <button class="cx-icon" data-graph-action="fit" title="Fit graph">⌖</button>
                  <span class="cx-hud-sep"></span>
                  <span>call</span><i class="cx-line"></i><span>data</span>
                </div>
                <div class="minimap" id="cx-minimap"></div>
              </div>
            </div>
            ${drawer(variant)}
          </main>
          ${variant === 'minimal' ? '' : variant === 'impact' ? impactInspector() : variant === 'compare' ? compareInspector() : variant === 'overview' ? overviewInspector() : focusInspector(variant === 'path' ? 'Path step' : variant === 'table' ? 'Selected row' : 'Focus')}
        </div>
        ${variant === 'palette' ? palette() : ''}
        ${variant === 'minimal' ? minimalInspector() : ''}
      </div>`;
  }

  function bind(variant, graph) {
    for (const button of $$('[data-graph-action="fit"]')) button.addEventListener('click', () => graph.fit());
    for (const button of $$('[data-node]')) {
      button.addEventListener('click', () => {
        graph.select(button.dataset.node);
        if (variant === 'minimal') showMinimalInspector();
      });
    }
    for (const row of $$('tr[data-node]')) {
      row.addEventListener('mouseenter', () => graph.select(row.dataset.node));
      row.addEventListener('focus', () => graph.select(row.dataset.node));
    }

    if (variant === 'path') {
      const steps = $$('[data-path-index]');
      let current = CG.checkoutPath.indexOf('createOrder');
      const go = (index) => {
        current = Math.max(0, Math.min(CG.checkoutPath.length - 1, index));
        const route = CG.checkoutPath.slice(0, current + 1);
        graph.route(route, CG.checkoutPath[current]);
        for (const step of steps) step.classList.toggle('active', Number(step.dataset.pathIndex) === current);
        for (const row of $$('tr[data-path-index]')) row.classList.toggle('selected', Number(row.dataset.pathIndex) === current);
      };
      for (const step of steps) step.addEventListener('click', () => go(Number(step.dataset.pathIndex)));
      for (const row of $$('tr[data-path-index]')) row.addEventListener('click', () => go(Number(row.dataset.pathIndex)));
      const play = $('[data-play]');
      let timer = null;
      play?.addEventListener('click', () => {
        if (timer) {
          clearInterval(timer); timer = null; play.textContent = '▶'; return;
        }
        play.textContent = '❚❚';
        timer = setInterval(() => {
          if (current >= CG.checkoutPath.length - 1) {
            clearInterval(timer); timer = null; play.textContent = '▶'; return;
          }
          go(current + 1);
        }, 900);
      });
      go(current);
    }

    if (variant === 'impact') {
      let extended = false;
      $('[data-impact-toggle]')?.addEventListener('click', (event) => {
        extended = !extended;
        event.currentTarget.textContent = extended ? 'tests: none' : 'tests: unknown';
        const affected = extended
          ? ['checkout', 'handleEvent', 'refund', 'legacyRefund']
          : ['checkout', 'handleEvent', 'refund'];
        graph.route([...affected, 'createOrder'], 'createOrder');
      });
      graph.route(['checkout', 'handleEvent', 'refund', 'legacyRefund', 'createOrder'], 'createOrder');
    }

    if (variant === 'compare') {
      let swapped = false;
      $('[data-swap]')?.addEventListener('click', () => {
        swapped = !swapped;
        const a = swapped ? 'validateCart' : 'createOrder';
        const b = swapped ? 'createOrder' : 'validateCart';
        $('[data-pin="a"]').textContent = qnameOf(a);
        $('[data-pin="b"]').textContent = qnameOf(b);
        const chips = $$('.cx-pin-chip');
        if (chips[0]) chips[0].textContent = `A · ${nameOf(a)}`;
        if (chips[1]) chips[1].textContent = `B · ${nameOf(b)}`;
        graph.route([...new Set([a, b, ...(CG.callers[a] || []), ...(CG.callers[b] || [])])], a);
      });
      graph.route([...new Set(['createOrder', 'validateCart', ...(CG.callers.createOrder || []), ...(CG.callers.validateCart || [])])], 'createOrder');
    }

    if (variant === 'table') {
      const toggle = () => {
        const drawer = $('#cx-drawer');
        if (!drawer) return;
        drawer.classList.toggle('open');
        for (const button of $$('[data-drawer-toggle]')) button.textContent = drawer.classList.contains('open') ? 'hide table' : 'show table';
        requestAnimationFrame(() => graph.fit());
      };
      for (const button of $$('[data-drawer-toggle]')) button.addEventListener('click', toggle);
      const search = $('#cx-table-search');
      search?.addEventListener('input', () => {
        const query = search.value.trim().toLowerCase();
        for (const row of $$('tr[data-node]')) {
          row.hidden = Boolean(query) && !row.textContent.toLowerCase().includes(query);
        }
      });
    }

    if (variant === 'palette') {
      const overlay = $('#cx-palette');
      const input = $('#cx-palette-input');
      const open = () => {
        overlay.hidden = false;
        input.value = '';
        filterPalette('');
        input.focus();
      };
      const close = () => { overlay.hidden = true; };
      const filterPalette = (query) => {
        const q = query.trim().toLowerCase();
        for (const item of $$('.cx-palette-item')) item.hidden = Boolean(q) && !item.textContent.toLowerCase().includes(q);
      };
      $('[data-palette-open]')?.addEventListener('click', open);
      input.addEventListener('input', () => filterPalette(input.value));
      overlay.addEventListener('click', (event) => { if (event.target === overlay) close(); });
      document.addEventListener('keydown', (event) => {
        if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); open(); }
        if (event.key === 'Escape') close();
      });
      for (const item of $$('.cx-palette-item[data-command]')) {
        item.addEventListener('click', () => {
          const command = item.dataset.command;
          if (command === 'resume') toast('Resumed snapshot a3f10c2');
          else if (command === 'json') toast('Open local graph.json');
          else if (command === 'focus') graph.select('createOrder');
          else if (command === 'impact') { location.href = PAGES.impact; return; }
          else if (command === 'table') { location.href = PAGES.table; return; }
          else if (command === 'export') toast('PNG export');
          close();
        });
      }
      open();
    }

    if (variant === 'overview') {
      for (const row of $$('[data-node]')) row.addEventListener('click', () => graph.select(row.dataset.node));
    }

    if (variant === 'minimal') {
      $('[data-min-close]')?.addEventListener('click', () => { $('#cx-min-inspect').hidden = true; });
    }

    const search = $('#cx-nav-search');
    search?.addEventListener('input', () => {
      const q = search.value.trim().toLowerCase();
      for (const row of $$('#cx-entry-list .cx-nav-row')) row.hidden = Boolean(q) && !row.textContent.toLowerCase().includes(q);
    });

    for (const row of $$('[data-scope], [data-recent]')) {
      row.addEventListener('click', () => toast(`${row.dataset.scope || row.dataset.recent} is a scope shortcut, not a separate home screen`));
    }

    document.addEventListener('keydown', (event) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.target instanceof Element && event.target.closest('input, textarea, select')) return;
      const key = event.key.toLowerCase();
      if (key === 'f') location.href = PAGES.focus;
      if (key === 'p') location.href = PAGES.path;
      if (key === 'i') location.href = PAGES.impact;
      if (key === 'c') location.href = PAGES.compare;
    });
  }

  function showMinimalInspector() {
    const panel = $('#cx-min-inspect');
    if (panel) panel.hidden = false;
  }

  function updateRelations(node) {
    const host = $('#cx-relations');
    if (host) host.innerHTML = relationsHtml(node.id);
    for (const target of $$('[data-selected-qname]')) target.textContent = `${node.cls}.${node.name}`;
    for (const target of $$('[data-selected-file]')) target.textContent = node.file;
  }

  function mount(root, variant) {
    const meta = META[variant];
    if (!meta) throw new Error(`unknown variant: ${variant}`);
    document.title = `${meta.number} · ${meta.title} — code-graph`;
    root.innerHTML = shell(variant);
    const graph = CG.mount($('#cx-graph'), {
      layout: variant === 'path' ? 'path' : 'hub',
      selected: variant === 'minimal' ? null : 'createOrder',
      maxScale: 1,
      onSelect: (node) => {
        updateRelations(node);
        if (variant === 'minimal') showMinimalInspector();
      },
    });
    CG.minimap($('#cx-minimap'), variant === 'path' ? 'path' : 'hub');
    bind(variant, graph);
    return graph;
  }

  window.Converge = { mount, pages: PAGES, meta: META };
})();
