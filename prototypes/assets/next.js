/* Small interaction helpers shared by the second prototype cut. */
(() => {
  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];

  function toast(message) {
    const el = document.createElement('div');
    el.className = 'nx-toast';
    el.textContent = message;
    document.body.appendChild(el);
    setTimeout(() => el.remove(), 1800);
  }

  function bindSelect(selector, graph, options = {}) {
    const items = $$(selector);
    items.forEach((item) => item.addEventListener('click', () => {
      items.forEach((node) => node.classList.remove('active'));
      item.classList.add('active');
      const id = item.dataset.select || item.dataset.id;
      if (id && graph) graph.select(id);
    }));
    return items;
  }

  function bindHover(selector, graph) {
    $$(selector).forEach((item) => item.addEventListener('mouseenter', () => {
      const id = item.dataset.select || item.dataset.id;
      if (id && graph) graph.select(id);
    }));
  }

  function bindFilter(input, rowSelector, count, total, label = (n) => `${n} / ${total}`) {
    if (!input) return;
    const rows = $$(rowSelector);
    input.addEventListener('input', () => {
      const query = input.value.trim().toLowerCase();
      let visible = 0;
      rows.forEach((row) => {
        const show = !query || row.textContent.toLowerCase().includes(query);
        row.style.display = show ? '' : 'none';
        if (show) visible++;
      });
      if (count) count.textContent = label(visible);
    });
  }

  function bindTabs(selector, onChange) {
    const tabs = $$(selector);
    tabs.forEach((tab) => tab.addEventListener('click', () => {
      tabs.forEach((item) => item.classList.remove('active'));
      tab.classList.add('active');
      if (onChange) onChange(tab);
    }));
    return tabs;
  }

  function playSteps(options) {
    const order = options.order || CG.checkoutPath;
    const steps = $$(options.steps);
    let index = Math.max(0, Math.min(order.length - 1, options.start ?? 0));
    let timer = null;
    const graph = options.graph;
    const onStep = options.onStep || (() => {});
    function go(next) {
      index = Math.max(0, Math.min(order.length - 1, next));
      steps.forEach((step, i) => {
        step.classList.toggle('active', i === index);
        step.classList.toggle('done', i < index);
      });
      if (graph) graph.route(order.slice(0, index + 1), order[index]);
      onStep(index, order[index]);
    }
    steps.forEach((step, i) => step.addEventListener('click', () => { stop(); go(i); }));
    function stop() { clearInterval(timer); timer = null; }
    function toggle() {
      if (timer) { stop(); return false; }
      if (index >= order.length - 1) go(0);
      timer = setInterval(() => {
        if (index >= order.length - 1) { stop(); return; }
        go(index + 1);
      }, options.interval || 1200);
      return true;
    }
    document.addEventListener('keydown', (event) => {
      if (event.key === 'ArrowRight') { stop(); go(index + 1); }
      if (event.key === 'ArrowLeft') { stop(); go(index - 1); }
    });
    go(index);
    return { go, toggle, stop, get index() { return index; } };
  }

  function bindCommand(input, selector, graph, options = {}) {
    if (!input) return;
    const items = $$(selector);
    input.addEventListener('input', () => {
      const query = input.value.trim().toLowerCase();
      items.forEach((item) => {
        item.style.display = !query || item.textContent.toLowerCase().includes(query) ? '' : 'none';
      });
    });
    items.forEach((item) => item.addEventListener('click', () => {
      const id = item.dataset.open || item.dataset.select;
      if (id && graph) graph.select(id);
      if (options.onOpen) options.onOpen(item, id);
      if (options.close) options.close();
    }));
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') {
        const first = items.find((item) => item.style.display !== 'none');
        if (first) first.click();
      }
      if (event.key === 'Escape' && options.close) options.close();
    });
    document.addEventListener('keydown', (event) => {
      if (event.key === '/' && document.activeElement !== input) { event.preventDefault(); input.focus(); }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); input.focus(); }
    });
  }

  window.NX = { $, $$, toast, bindSelect, bindHover, bindFilter, bindTabs, playSteps, bindCommand };
})();
