/* One synthesized prototype: the current three-column web shell with
   Focus / Path / Impact / Compare modes. Feedback is structured only. */
(() => {
  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
  const esc = (value) => String(value ?? '').replace(/[&<>"']/g, (char) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[char]));
  const nodeName = (id) => CG.byId[id]?.name || id;
  const nodeQname = (id) => `${CG.byId[id]?.cls || ''}.${CG.byId[id]?.name || id}`;

  const state = {
    mode: 'focus',
    selected: 'createOrder',
    pathIndex: 3,
    compareA: 'createOrder',
    compareB: 'validateCart',
    impactExtended: false,
  };
  let graph = null;
  let applyingGraph = false;
  let playTimer = null;

  function header() {
    return `
      <header class="wb-header">
        <div class="wb-brand"><span class="wb-mark">◉</span><span>code-graph</span><span class="wb-tag">LSP call graph</span></div>
        <div class="wb-meta">
          <span class="wb-root">checkout-service</span><i>·</i><span>typescript</span><i>·</i>
          <span class="wb-chip">148 files</span><span class="wb-chip">2,462 symbols</span>
          <span class="wb-chip">5,840 edges</span><span class="wb-chip">12 signals</span>
          <span class="wb-chip danger">3 cycles</span>
        </div>
        <div class="wb-actions">
          <span class="wb-status-dot"></span><span class="wb-local">snapshot a3f10c2 · local</span>
          <button class="wb-feedback-open" data-feedback-open>Оценить прототип</button>
        </div>
      </header>`;
  }

  function sidebar() {
    const nodes = [...CG.nodes].sort((a, b) => b.hotspot - a.hotspot).slice(0, 16);
    return `
      <aside class="wb-sidebar">
        <div class="wb-side-head"><span>All nodes</span><span class="wb-faint">160 / 160</span></div>
        <div class="wb-sort"><button class="active">hotspot</button><button>fan-in</button><button>fan-out</button><button>name</button></div>
        <div class="wb-node-list">
          ${nodes.map((node) => `
            <button class="wb-node-row" data-node="${esc(node.id)}">
              <span class="wb-node-main"><b>${esc(node.cls)}.${esc(node.name)}</b><span>${esc(node.kind)} · ${esc(node.file)}</span></span>
              <span class="wb-node-nums"><span>${node.fanIn}</span><span>${node.fanOut}</span><span>${node.hotspot}</span></span>
            </button>`).join('')}
        </div>
        <div class="wb-side-section">
          <div class="wb-side-label">Filters</div>
          <label class="wb-check"><input type="checkbox" checked> function</label>
          <label class="wb-check"><input type="checkbox" checked> method</label>
          <label class="wb-check"><input type="checkbox"> external</label>
          <label class="wb-check"><input type="checkbox"> dead-code candidates</label>
        </div>
        <div class="wb-side-section">
          <div class="wb-side-label">Layout</div>
          <div class="wb-layout-row"><button class="active">Layered</button><button>Stress</button><button>Grid</button></div>
        </div>
        <div class="wb-side-foot"><span class="wb-status-dot"></span>local JSON · no API</div>
      </aside>`;
  }

  function modebar() {
    return `
      <div class="wb-modebar">
        <span class="wb-mode-label">Mode</span>
        <div class="wb-modes">
          ${[
            ['focus', 'Focus'],
            ['path', 'Path'],
            ['impact', 'Impact'],
            ['compare', 'Compare'],
          ].map(([id, label]) => `<button class="wb-mode" data-mode="${id}"><i class="wb-mode-dot ${id}"></i>${label}</button>`).join('')}
        </div>
        <span class="wb-mode-hint" data-mode-hint></span>
        <span class="wb-spacer"></span>
        <span class="wb-mode-count" data-mode-count></span>
      </div>`;
  }

  function context() {
    if (state.mode === 'focus') {
      return `<div class="wb-context"><span class="wb-context-label">Focus</span><span class="wb-context-note">select a node · path, impact and compare stay inside this canvas</span><span class="wb-spacer"></span><span class="wb-context-note">20 nodes · 23 edges</span></div>`;
    }
    if (state.mode === 'path') {
      return `
        <div class="wb-context wb-path-context">
          <span class="wb-context-label">Path</span>
          <div class="wb-path-strip">
            ${CG.checkoutPath.map((id, index) => `<button class="wb-path-step ${index === state.pathIndex ? 'active' : ''}" data-path-index="${index}"><span>${index + 1}</span>${esc(nodeName(id))}</button>${index < CG.checkoutPath.length - 1 ? '<i>→</i>' : ''}`).join('')}
          </div>
          <button class="wb-icon" data-play title="Play path">${playTimer ? '❚❚' : '▶'}</button>
        </div>`;
    }
    if (state.mode === 'impact') {
      const callers = CG.callers[state.selected] || [];
      return `
        <div class="wb-context">
          <span class="wb-context-label">Impact · reverse</span>
          <b class="wb-mono">${esc(nodeQname(state.selected))}</b>
          <span class="wb-chip">${callers.length} direct</span>
          <span class="wb-chip danger">risk 0.62</span>
          <button class="wb-icon" data-impact-toggle title="Toggle transitive callers">+</button>
          <span class="wb-context-note">${state.impactExtended ? 'transitive callers included' : 'direct callers only'}</span>
        </div>`;
    }
    return `
      <div class="wb-context">
        <span class="wb-context-label">Compare · pinned</span>
        <span class="wb-pin-chip">A · ${esc(nodeName(state.compareA))}</span><i>vs</i>
        <span class="wb-pin-chip">B · ${esc(nodeName(state.compareB))}</span>
        <button class="wb-icon" data-swap title="Swap pins">⇄</button>
      </div>`;
  }

  function factsFor(node) {
    return `
      <div class="wb-facts">
        <div><b>${node.fanIn}</b><span>fan-in</span></div>
        <div><b>${node.fanOut}</b><span>fan-out</span></div>
        <div><b>${node.hotspot}</b><span>hotspot</span></div>
        <div><b>${node.flags?.includes('entry') ? 'entry' : '—'}</b><span>role</span></div>
      </div>`;
  }

  function relationsFor(id) {
    const callers = (CG.callers[id] || []).slice(0, 5).map((source) => `<button class="wb-rel" data-node="${esc(source)}"><span>←</span><b>${esc(nodeName(source))}</b><small>caller</small></button>`).join('');
    const callees = (CG.callees[id] || []).slice(0, 5).map((target) => `<button class="wb-rel" data-node="${esc(target)}"><span>→</span><b>${esc(nodeName(target))}</b><small>callee</small></button>`).join('');
    return `${callers}${callees}` || '<div class="wb-muted">No direct relations.</div>';
  }

  function details() {
    const node = CG.byId[state.selected] || CG.byId.createOrder;
    if (state.mode === 'impact') {
      return `
        <div class="wb-details-head"><div class="wb-details-label">Impact</div><h2>${esc(nodeQname(state.selected))}</h2><div class="wb-file">${esc(node.file)}</div></div>
        <div class="wb-risk"><b>0.62</b><span>risk · ${(CG.callers[state.selected] || []).length} direct callers</span></div>
        <div class="wb-risk-meter"><i></i></div>
        <div class="wb-details-section"><div class="wb-details-label">Affected</div>
          ${(CG.callers[state.selected] || []).map((id) => `<button class="wb-rel" data-node="${esc(id)}"><span>←</span><b>${esc(nodeName(id))}</b><small>${CG.byId[id]?.flags?.includes('entry') ? 'entry' : 'caller'}</small></button>`).join('') || '<div class="wb-muted">No callers.</div>'}
        </div>
        <div class="wb-details-section"><div class="wb-details-label">Evidence</div>
          <div class="wb-evidence"><b>No direct tests</b><span>inferred from call edges</span></div>
          <div class="wb-evidence"><b>No cycle</b><span>shortest entry distance 2</span></div>
        </div>
        <div class="wb-details-actions"><button data-mode="compare">Compare with…</button><button data-mode="path">Open path ↦</button></div>`;
    }
    if (state.mode === 'compare') {
      const a = CG.byId[state.compareA];
      const b = CG.byId[state.compareB];
      const shared = (CG.callers[a.id] || []).filter((id) => (CG.callers[b.id] || []).includes(id));
      const onlyA = (CG.callees[a.id] || []).filter((id) => !(CG.callees[b.id] || []).includes(id));
      const onlyB = (CG.callees[b.id] || []).filter((id) => !(CG.callees[a.id] || []).includes(id));
      return `
        <div class="wb-details-head"><div class="wb-details-label">Compare</div><h2>Two pins</h2><div class="wb-file">shared graph, separate context</div></div>
        <div class="wb-compare-card"><span>A</span><h3>${esc(nodeQname(a.id))}</h3><div class="wb-file">${esc(a.file)}</div><div class="wb-mini-nums">in ${a.fanIn} · out ${a.fanOut} · hotspot ${a.hotspot}</div></div>
        <div class="wb-compare-card"><span>B</span><h3>${esc(nodeQname(b.id))}</h3><div class="wb-file">${esc(b.file)}</div><div class="wb-mini-nums">in ${b.fanIn} · out ${b.fanOut} · hotspot ${b.hotspot}</div></div>
        <div class="wb-details-section"><div class="wb-details-label">Diff</div>
          <div class="wb-diff"><span>Shared callers</span><b>${shared.map(nodeName).join(', ') || '—'}</b></div>
          <div class="wb-diff"><span>Only A</span><b>${onlyA.map(nodeName).join(', ') || '—'}</b></div>
          <div class="wb-diff"><span>Only B</span><b>${onlyB.map(nodeName).join(', ') || '—'}</b></div>
        </div>
        <div class="wb-details-actions"><button data-swap>Swap pins ⇄</button><button data-mode="focus">Back to focus</button></div>`;
    }
    if (state.mode === 'path') {
      const pathId = CG.checkoutPath[state.pathIndex];
      const pathNode = CG.byId[pathId];
      return `
        <div class="wb-details-head"><div class="wb-details-label">Path · step ${state.pathIndex + 1} / ${CG.checkoutPath.length}</div><h2>${esc(nodeQname(pathId))}</h2><div class="wb-file">${esc(pathNode.file)}</div></div>
        ${factsFor(pathNode)}
        <div class="wb-details-section"><div class="wb-details-label">Route</div>
          ${CG.checkoutPath.map((id, index) => `<button class="wb-rel ${index === state.pathIndex ? 'active' : ''}" data-path-index="${index}"><span>${index + 1}</span><b>${esc(nodeName(id))}</b><small>${index < state.pathIndex ? 'done' : index === state.pathIndex ? 'current' : 'next'}</small></button>`).join('')}
        </div>
        <div class="wb-details-section"><div class="wb-details-label">Relations</div>${relationsFor(pathId)}</div>
        <div class="wb-details-actions"><button data-mode="impact">Show impact ←</button><button data-mode="focus">Back to focus</button></div>`;
    }
    return `
      <div class="wb-details-head"><div class="wb-details-label">Focus</div><h2>${esc(nodeQname(state.selected))}</h2><div class="wb-file">${esc(node.file)}</div></div>
      ${factsFor(node)}
      <div class="wb-details-section"><div class="wb-details-label">Relations</div><div class="wb-relations">${relationsFor(state.selected)}</div></div>
      <div class="wb-details-section"><div class="wb-details-label">Modes</div>
        <div class="wb-details-actions"><button data-mode="path">Path entry → exit</button><button data-mode="impact">Blast radius</button><button data-mode="compare">Pin compare</button></div>
      </div>
      <div class="wb-details-foot"><span class="wb-status-dot"></span>snapshot a3f10c2 · local · 6 min ago</div>`;
  }

  function feedback() {
    return `
      <div class="wb-feedback" id="wb-feedback" hidden>
        <div class="wb-feedback-box" role="dialog" aria-modal="true" aria-label="Оценить прототип">
          <div class="wb-feedback-head">
            <div><div class="wb-details-label">Structured feedback</div><h2>Оценить прототип</h2><p>Только выборы. Никакого открытого текста.</p></div>
            <button class="wb-icon" data-feedback-close aria-label="Закрыть">×</button>
          </div>
          <form id="wb-feedback-form">
            <fieldset><legend>Режимы в следующем обновлении</legend>
              <label><input type="checkbox" name="modes" value="path" checked> Path</label>
              <label><input type="checkbox" name="modes" value="impact" checked> Impact</label>
              <label><input type="checkbox" name="modes" value="compare" checked> Compare</label>
            </fieldset>
            <fieldset><legend>Левая таблица</legend>
              <label><input type="radio" name="table" value="keep" checked> оставить как есть</label>
              <label><input type="radio" name="table" value="collapse"> свернуть в drawer</label>
              <label><input type="radio" name="table" value="remove"> убрать</label>
            </fieldset>
            <fieldset><legend>Правая панель</legend>
              <label><input type="radio" name="inspector" value="details" checked> детали выбранного</label>
              <label><input type="radio" name="inspector" value="mixed"> детали + overview</label>
              <label><input type="radio" name="inspector" value="overview"> overview</label>
            </fieldset>
            <fieldset><legend>Плотность</legend>
              <label><input type="radio" name="density" value="compact"> компактная</label>
              <label><input type="radio" name="density" value="current" checked> текущая</label>
              <label><input type="radio" name="density" value="airy"> воздушная</label>
            </fieldset>
            <fieldset><legend>Что делать следующим</legend>
              <label><input type="checkbox" name="next" value="path" checked> Path</label>
              <label><input type="checkbox" name="next" value="impact" checked> Impact</label>
              <label><input type="checkbox" name="next" value="compare"> Compare</label>
              <label><input type="checkbox" name="next" value="table"> Table polish</label>
              <label><input type="checkbox" name="next" value="palette"> Command palette</label>
            </fieldset>
            <fieldset class="wb-rating"><legend>Общая оценка</legend>
              <label><input type="radio" name="overall" value="1">1</label>
              <label><input type="radio" name="overall" value="2">2</label>
              <label><input type="radio" name="overall" value="3">3</label>
              <label><input type="radio" name="overall" value="4" checked>4</label>
              <label><input type="radio" name="overall" value="5">5</label>
            </fieldset>
            <div class="wb-feedback-actions">
              <span class="wb-feedback-error" data-feedback-error></span>
              <button type="button" class="wb-button" data-feedback-close>Отмена</button>
              <button type="submit" class="wb-button primary">Отправить</button>
            </div>
          </form>
          <div class="wb-feedback-done" id="wb-feedback-done" hidden>
            <h2>Фидбек сохранён</h2>
            <p>Структурированные ответы записаны. Спасибо.</p>
            <button class="wb-button" data-feedback-again>Изменить</button>
          </div>
        </div>
      </div>`;
  }

  function shell() {
    return `
      <div class="wb-app">
        ${header()}
        ${sidebar()}
        <main class="wb-main">
          ${modebar()}
          <div id="wb-context">${context()}</div>
          <div class="wb-canvas-wrap">
            <div class="wb-canvas graph-shell" id="wb-graph">
              <div class="wb-hud"><button class="wb-icon" data-fit title="Fit graph">⌖</button><span class="wb-hud-sep"></span><span>call</span><i class="wb-line"></i><span>data</span></div>
              <div class="minimap" id="wb-minimap"></div>
            </div>
          </div>
          <div class="wb-bottom"><span><b>20</b> nodes · <b>23</b> edges</span><span class="wb-spacer"></span><span>F focus · P path · I impact · C compare</span><span class="wb-bottom-ready">local</span></div>
        </main>
        <aside class="wb-details" id="wb-details">${details()}</aside>
        <button class="wb-feedback-float" data-feedback-open>Оценить</button>
        ${feedback()}
      </div>`;
  }

  function updateModebar() {
    for (const button of $$('[data-mode]')) button.classList.toggle('active', button.dataset.mode === state.mode);
    const hints = {
      focus: 'one node, local context',
      path: 'entry → exit, step by step',
      impact: 'reverse callers and risk',
      compare: 'two pins, one graph',
    };
    const hint = $('[data-mode-hint]');
    if (hint) hint.textContent = hints[state.mode];
    const count = $('[data-mode-count]');
    if (count) count.textContent = state.mode === 'path' ? `${state.pathIndex + 1} / ${CG.checkoutPath.length}` : state.mode === 'compare' ? '2 pins' : state.mode === 'impact' ? `${(CG.callers[state.selected] || []).length} callers` : '20 nodes';
  }

  function updateSidebar() {
    for (const row of $$('.wb-node-row')) row.classList.toggle('active', row.dataset.node === state.selected);
  }

  function updateDetails() {
    const host = $('#wb-details');
    if (host) host.innerHTML = details();
  }

  function updateContext() {
    const host = $('#wb-context');
    if (host) host.innerHTML = context();
  }

  function applyGraph() {
    if (!graph) return;
    applyingGraph = true;
    try {
    if (state.mode === 'focus') {
      graph.select(state.selected);
      return;
    }
    if (state.mode === 'path') {
      const route = CG.checkoutPath.slice(0, state.pathIndex + 1);
      graph.route(route, CG.checkoutPath[state.pathIndex]);
      return;
    }
    if (state.mode === 'impact') {
      const direct = CG.callers[state.selected] || [];
      const set = new Set([...direct, state.selected]);
      if (state.impactExtended) {
        for (const caller of direct) {
          for (const second of CG.callers[caller] || []) set.add(second);
        }
      }
      graph.route([...set], state.selected);
      return;
    }
    const a = CG.byId[state.compareA];
    const b = CG.byId[state.compareB];
      if (a && b) {
        graph.route([...new Set([a.id, b.id, ...(CG.callers[a.id] || []), ...(CG.callers[b.id] || [])])], a.id);
      }
    } finally {
      applyingGraph = false;
    }
  }

  function render() {
    updateModebar();
    updateSidebar();
    updateContext();
    updateDetails();
  }

  function setMode(mode) {
    state.mode = mode;
    if (mode !== 'path' && playTimer) { clearInterval(playTimer); playTimer = null; }
    if (mode === 'path' && !CG.checkoutPath.includes(state.selected)) {
      state.pathIndex = CG.checkoutPath.indexOf('createOrder');
    }
    if (mode === 'path') {
      const index = CG.checkoutPath.indexOf(state.selected);
      if (index >= 0) state.pathIndex = index;
    }
    if (mode === 'compare' && state.compareA === state.compareB) state.compareB = 'validateCart';
    applyGraph();
    render();
  }

  function chooseNode(id) {
    if (!CG.byId[id]) return;
    state.selected = id;
    if (state.mode === 'path') {
      const index = CG.checkoutPath.indexOf(id);
      if (index >= 0) state.pathIndex = index;
    }
    if (state.mode === 'compare') state.compareA = id;
    applyGraph();
    render();
  }

  function openFeedback() {
    const modal = $('#wb-feedback');
    if (!modal) return;
    modal.hidden = false;
    $('#wb-feedback-form').hidden = false;
    $('#wb-feedback-done').hidden = true;
    const input = modal.querySelector('input[name="overall"]');
    if (input) input.focus();
  }

  function closeFeedback() {
    const modal = $('#wb-feedback');
    if (modal) modal.hidden = true;
  }

  function gatherFeedback(form) {
    const data = new FormData(form);
    return {
      modes: data.getAll('modes'),
      table: data.get('table') || '',
      inspector: data.get('inspector') || '',
      density: data.get('density') || '',
      next: data.getAll('next'),
      overall: Number(data.get('overall') || 0),
      prototype: 'c09-mode-workbench',
      submittedAt: new Date().toISOString(),
    };
  }

  async function submitFeedback(event) {
    event.preventDefault();
    const form = event.currentTarget;
    const error = $('[data-feedback-error]');
    const payload = gatherFeedback(form);
    if (!payload.overall) {
      if (error) error.textContent = 'Выберите общую оценку.';
      return;
    }
    if (error) error.textContent = '';
    try {
      const response = await fetch('/api/converge-feedback', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      if (!response.ok) throw new Error(`${response.status}`);
      form.hidden = true;
      $('#wb-feedback-done').hidden = false;
      toast('Фидбек сохранён · спасибо');
    } catch (caught) {
      if (error) error.textContent = `Не удалось отправить: ${caught.message}`;
    }
  }

  function toast(message) {
    const node = document.createElement('div');
    node.className = 'wb-toast';
    node.textContent = message;
    document.body.appendChild(node);
    setTimeout(() => node.remove(), 2200);
  }

  function togglePlay() {
    if (playTimer) {
      clearInterval(playTimer);
      playTimer = null;
      const button = $('[data-play]');
      if (button) button.textContent = '▶';
      return;
    }
    const button = $('[data-play]');
    if (!button) return;
    button.textContent = '❚❚';
    playTimer = setInterval(() => {
      const current = $('[data-play]');
      if (state.pathIndex >= CG.checkoutPath.length - 1) {
        clearInterval(playTimer); playTimer = null;
        if (current) current.textContent = '▶';
        return;
      }
      state.pathIndex += 1;
      state.selected = CG.checkoutPath[state.pathIndex];
      applyGraph(); render();
    }, 900);
  }

  function bind() {
    document.addEventListener('click', (event) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const mode = target.closest('[data-mode]');
      if (mode) { setMode(mode.dataset.mode); return; }
      const node = target.closest('[data-node]');
      if (node) { chooseNode(node.dataset.node); return; }
      const step = target.closest('[data-path-index]');
      if (step) {
        state.pathIndex = Number(step.dataset.pathIndex);
        state.selected = CG.checkoutPath[state.pathIndex];
        applyGraph(); render(); return;
      }
      if (target.closest('[data-swap]')) {
        [state.compareA, state.compareB] = [state.compareB, state.compareA];
        applyGraph(); render(); return;
      }
      if (target.closest('[data-fit]')) { graph.fit(); return; }
      if (target.closest('[data-impact-toggle]')) {
        state.impactExtended = !state.impactExtended;
        applyGraph(); render(); return;
      }
      if (target.closest('[data-play]')) { togglePlay(); return; }
      if (target.closest('[data-feedback-open]')) { openFeedback(); return; }
      if (target.closest('[data-feedback-close]')) { closeFeedback(); return; }
      if (target.closest('[data-feedback-again]')) {
        $('#wb-feedback-form').hidden = false;
        $('#wb-feedback-done').hidden = true;
        return;
      }
    });

    $('#wb-feedback-form')?.addEventListener('submit', submitFeedback);

    document.addEventListener('keydown', (event) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.target instanceof Element && event.target.closest('input, textarea, select')) return;
      const key = event.key.toLowerCase();
      if (key === 'f') setMode('focus');
      if (key === 'p') setMode('path');
      if (key === 'i') setMode('impact');
      if (key === 'c') setMode('compare');
      if (key === 'escape') closeFeedback();
    });
  }

  function mount(root) {
    root.innerHTML = shell();
    graph = CG.mount($('#wb-graph'), {
      layout: 'hub',
      selected: state.selected,
      maxScale: 1,
      onSelect: (node) => {
        if (applyingGraph) return;
        state.selected = node.id;
        if (state.mode === 'compare') state.compareA = node.id;
        if (state.mode === 'path') {
          const index = CG.checkoutPath.indexOf(node.id);
          if (index >= 0) state.pathIndex = index;
        }
        if (state.mode !== 'focus') applyGraph();
        render();
      },
    });
    CG.minimap($('#wb-minimap'), 'hub');
    bind();
    render();
    applyGraph();
    return graph;
  }

  window.Workbench = { mount };
})();
