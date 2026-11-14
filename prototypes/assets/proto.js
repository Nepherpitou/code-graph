/* Shared demo graph: a small but realistic TypeScript checkout service.
   No dependencies; every prototype uses the same data so screenshots compare. */
(() => {
  const project = {
    name: 'checkout-service',
    repo: 'acme/checkout',
    root: '~/work/acme/checkout',
    language: 'TypeScript',
    server: 'tsgo 5.9',
    branch: 'main',
    commit: 'a3f10c2',
    analyzed: '6 min ago',
    duration: '8.4s',
    files: 148,
    symbols: 2462,
    edges: 5840,
    findings: 12,
    cycles: 3,
    dead: 7,
  };

  const nodes = [
    { id: 'checkout', name: 'checkout', cls: 'CheckoutController', kind: 'method', file: 'src/http/CheckoutController.ts', fanIn: 1, fanOut: 5, hotspot: 94, flags: ['entry'], x: 30, y: 262 },
    { id: 'webhook', name: 'handle', cls: 'PaymentWebhook', kind: 'method', file: 'src/payments/PaymentWebhook.ts', fanIn: 0, fanOut: 2, hotspot: 61, flags: ['entry'], x: 30, y: 76 },
    { id: 'validateCart', name: 'validateCart', cls: 'CheckoutController', kind: 'method', file: 'src/http/CheckoutController.ts', fanIn: 3, fanOut: 2, hotspot: 38, x: 230, y: 176 },
    { id: 'handlePaymentError', name: 'handlePaymentError', cls: 'CheckoutController', kind: 'method', file: 'src/http/CheckoutController.ts', fanIn: 1, fanOut: 2, hotspot: 42, flags: ['async'], x: 230, y: 378 },
    { id: 'verifySignature', name: 'verifySignature', cls: 'PaymentWebhook', kind: 'function', file: 'src/payments/PaymentWebhook.ts', fanIn: 1, fanOut: 0, hotspot: 18, x: 230, y: 68 },
    { id: 'handleEvent', name: 'handleEvent', cls: 'PaymentWebhook', kind: 'function', file: 'src/payments/PaymentWebhook.ts', fanIn: 1, fanOut: 2, hotspot: 47, x: 230, y: 274 },
    { id: 'priceCart', name: 'priceCart', cls: 'PricingService', kind: 'method', file: 'src/domain/PricingService.ts', fanIn: 4, fanOut: 3, hotspot: 71, x: 430, y: 102 },
    { id: 'createOrder', name: 'createOrder', cls: 'OrderService', kind: 'method', file: 'src/domain/OrderService.ts', fanIn: 3, fanOut: 4, hotspot: 88, flags: ['hot'], x: 430, y: 266 },
    { id: 'refund', name: 'refund', cls: 'RefundService', kind: 'method', file: 'src/domain/RefundService.ts', fanIn: 2, fanOut: 1, hotspot: 56, x: 430, y: 438 },
    { id: 'applyCoupon', name: 'applyCoupon', cls: 'PricingService', kind: 'method', file: 'src/domain/PricingService.ts', fanIn: 1, fanOut: 1, hotspot: 22, x: 630, y: 42 },
    { id: 'taxFor', name: 'taxFor', cls: 'PricingService', kind: 'function', file: 'src/domain/PricingService.ts', fanIn: 2, fanOut: 0, hotspot: 14, x: 630, y: 148 },
    { id: 'reserveInventory', name: 'reserveInventory', cls: 'InventoryService', kind: 'method', file: 'src/domain/InventoryService.ts', fanIn: 1, fanOut: 1, hotspot: 35, x: 630, y: 232 },
    { id: 'authorize', name: 'authorize', cls: 'PaymentGateway', kind: 'method', file: 'src/payments/PaymentGateway.ts', fanIn: 1, fanOut: 2, hotspot: 68, flags: ['async'], x: 630, y: 330 },
    { id: 'paymentRepo', name: 'PaymentRepository', cls: 'stripe', kind: 'external', file: 'src/payments/PaymentRepository.ts', fanIn: 2, fanOut: 0, hotspot: 9, x: 630, y: 436 },
    { id: 'capture', name: 'capture', cls: 'PaymentGateway', kind: 'method', file: 'src/payments/PaymentGateway.ts', fanIn: 1, fanOut: 1, hotspot: 29, x: 830, y: 298 },
    { id: 'finalizeOrder', name: 'finalizeOrder', cls: 'OrderService', kind: 'method', file: 'src/domain/OrderService.ts', fanIn: 1, fanOut: 1, hotspot: 31, x: 830, y: 200 },
    { id: 'emitEvent', name: 'emitEvent', cls: 'EventBus', kind: 'function', file: 'src/infra/EventBus.ts', fanIn: 1, fanOut: 0, hotspot: 11, x: 830, y: 414 },
    { id: 'inventoryRepo', name: 'InventoryRepository', cls: 'postgres', kind: 'external', file: 'src/domain/InventoryRepository.ts', fanIn: 1, fanOut: 0, hotspot: 7, x: 830, y: 92 },
    { id: 'log', name: 'logger', cls: 'pino', kind: 'external', file: 'src/lib/logger.ts', fanIn: 2, fanOut: 0, hotspot: 4, x: 830, y: 512 },
    { id: 'legacyRefund', name: 'legacyRefund', cls: 'RefundService', kind: 'method', file: 'src/domain/RefundService.ts', fanIn: 0, fanOut: 0, hotspot: 2, flags: ['dead'], x: 430, y: 530 },
  ];

  const edges = [
    ['checkout', 'validateCart'], ['checkout', 'createOrder'], ['checkout', 'priceCart'],
    ['checkout', 'handlePaymentError'], ['createOrder', 'reserveInventory'],
    ['createOrder', 'authorize'], ['createOrder', 'finalizeOrder'],
    ['validateCart', 'priceCart'], ['priceCart', 'applyCoupon'], ['priceCart', 'taxFor'],
    ['applyCoupon', 'taxFor', 'data'], ['authorize', 'capture'], ['authorize', 'log'],
    ['reserveInventory', 'inventoryRepo'], ['capture', 'paymentRepo'],
    ['handlePaymentError', 'refund'], ['refund', 'paymentRepo'],
    ['webhook', 'verifySignature'], ['webhook', 'handleEvent'],
    ['handleEvent', 'createOrder'], ['handleEvent', 'refund'],
    ['finalizeOrder', 'emitEvent'], ['legacyRefund', 'refund'],
  ].map(([from, to, kind], index) => ({ id: `${from}-${to}`, from, to, kind: kind || 'call', index }));

  const modules = [
    { id: 'http', label: 'HTTP / controllers', x: 14, y: 34, w: 250, h: 310, tone: '#6ea8ff' },
    { id: 'domain', label: 'Domain', x: 280, y: 34, w: 380, h: 310, tone: '#b79cff' },
    { id: 'payments', label: 'Payments', x: 676, y: 34, w: 310, h: 310, tone: '#f0a35e' },
    { id: 'infra', label: 'Infrastructure', x: 280, y: 370, w: 590, h: 190, tone: '#58d6a7' },
  ];

  const layouts = {
    layered: twin(nodes, (n) => [n.x, n.y]),
    hub: {
      createOrder: [420, 270], checkout: [220, 270], webhook: [220, 86], validateCart: [220, 388],
      handleEvent: [220, 486], handlePaymentError: [220, 520], priceCart: [620, 160],
      authorize: [620, 270], reserveInventory: [620, 380], finalizeOrder: [620, 470],
      capture: [820, 270], refund: [620, 520], paymentRepo: [820, 380],
      applyCoupon: [820, 160], taxFor: [820, 70], inventoryRepo: [820, 480],
      log: [820, 520], emitEvent: [820, 520], verifySignature: [60, 86],
      legacyRefund: [60, 520],
    },
    modules: {
      checkout: [38, 150], webhook: [38, 38], validateCart: [190, 150],
      handlePaymentError: [38, 260], handleEvent: [190, 260],
      priceCart: [305, 120], createOrder: [480, 120], refund: [480, 250],
      finalizeOrder: [305, 250], applyCoupon: [305, 38], taxFor: [480, 38],
      authorize: [700, 110], capture: [820, 110], paymentRepo: [700, 220],
      inventoryRepo: [700, 38], reserveInventory: [480, 360], emitEvent: [600, 430],
      log: [800, 430], legacyRefund: [305, 430], verifySignature: [38, 38],
    },
    path: {
      checkout: [28, 266], validateCart: [220, 266], priceCart: [412, 266],
      applyCoupon: [604, 168], taxFor: [796, 168], createOrder: [604, 356],
      reserveInventory: [796, 356], authorize: [604, 470], capture: [796, 470],
      paymentRepo: [604, 520], finalizeOrder: [412, 470], emitEvent: [220, 470],
      handlePaymentError: [412, 62], refund: [604, 62], handleEvent: [28, 62],
      webhook: [28, 430], verifySignature: [28, 520], inventoryRepo: [796, 62],
      log: [796, 520], legacyRefund: [604, 520],
    },
    radial: {
      createOrder: [420, 246], checkout: [236, 246], priceCart: [420, 98],
      validateCart: [236, 98], authorize: [604, 168], capture: [760, 246],
      refund: [604, 390], handlePaymentError: [420, 458], handleEvent: [90, 170],
      webhook: [90, 330], finalizeOrder: [604, 98], reserveInventory: [604, 458],
      paymentRepo: [860, 330], inventoryRepo: [760, 98], emitEvent: [236, 458],
      taxFor: [760, 458], applyCoupon: [420, 10], log: [860, 458],
      verifySignature: [90, 458], legacyRefund: [860, 10],
    },
  };

  function twin(items, mapper) {
    return Object.fromEntries(items.map((item) => [item.id, mapper(item)]));
  }

  const byId = Object.fromEntries(nodes.map((node) => [node.id, node]));
  const kindLabel = { function: 'fn', method: 'method', constructor: 'ctor', constant: 'const', external: 'external' };
  const counters = { graph: 0 };

  function nodeEl(node) {
    const el = document.createElement('button');
    el.className = `gnode k-${node.kind}`;
    el.type = 'button';
    el.dataset.id = node.id;
    el.setAttribute('aria-label', `${node.cls}.${node.name}, ${node.kind}`);
    const flags = (node.flags || []).map((flag) => `<span class="g-flag ${flag === 'dead' ? 'danger' : flag === 'hot' || flag === 'async' ? 'warn' : ''}">${flag}</span>`).join('');
    el.innerHTML = `
      <span class="g-flags">${flags}</span>
      <span class="g-title">${node.name}</span>
      <span class="g-sub">${node.cls} · ${kindLabel[node.kind] || node.kind}</span>
    `;
    return el;
  }

  function pathFor(a, b) {
    const x1 = a.x + 160;
    const y1 = a.y + 26;
    const x2 = b.x;
    const y2 = b.y + 26;
    const dx = Math.max(36, Math.abs(x2 - x1) * .48);
    if (x2 < x1) {
      return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
    }
    return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
  }

  function mount(el, options = {}) {
    const layout = layouts[options.layout || 'layered'] || layouts.layered;
    const selectedStart = options.selected || null;
    const hidden = new Set(options.hidden || []);
    const visible = nodes.filter((node) => !hidden.has(node.id));
    const fitMax = options.maxScale ?? 1;
    const svgId = `arrow${++counters.graph}`;
    const world = document.createElement('div');
    world.className = 'graph-world';
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', '0 0 1000 600');
    svg.setAttribute('aria-hidden', 'true');
    svg.innerHTML = `<defs><marker id="${svgId}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto"><path class="arrow" d="M0,0 L8,4 L0,8 z"/></marker></defs>`;
    const edgeLayer = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    edgeLayer.setAttribute('class', 'graph-edges');
    svg.appendChild(edgeLayer);
    world.appendChild(svg);

    if (options.clusters) {
      for (const cluster of modules) {
        const box = document.createElement('div');
        box.className = 'g-cluster';
        box.style.cssText = `left:${cluster.x}px;top:${cluster.y}px;width:${cluster.w}px;height:${cluster.h}px;border-color:color-mix(in srgb, ${cluster.tone} 30%, transparent);`;
        box.innerHTML = `<span class="g-cluster-label" style="color:${cluster.tone}">${cluster.label}</span>`;
        world.appendChild(box);
      }
    }

    const edgeEls = [];
    for (const edge of edges) {
      if (hidden.has(edge.from) || hidden.has(edge.to)) continue;
      const a = byId[edge.from];
      const b = byId[edge.to];
      const pa = layout[edge.from] || [a.x, a.y];
      const pb = layout[edge.to] || [b.x, b.y];
      const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      path.setAttribute('d', pathFor({ x: pa[0], y: pa[1] }, { x: pb[0], y: pb[1] }));
      path.setAttribute('marker-end', `url(#${svgId})`);
      path.dataset.from = edge.from;
      path.dataset.to = edge.to;
      if (edge.kind === 'data') path.classList.add('data');
      edgeEls.push(path);
      edgeLayer.appendChild(path);
    }

    const nodeEls = new Map();
    for (const node of visible) {
      const elNode = nodeEl(node);
      const pos = layout[node.id] || [node.x, node.y];
      elNode.style.left = `${pos[0]}px`;
      elNode.style.top = `${pos[1]}px`;
      world.appendChild(elNode);
      nodeEls.set(node.id, elNode);
    }
    el.appendChild(world);

    const state = { selected: null, api: null };
    function select(id) {
      state.selected = id;
      const neighbors = new Set([id]);
      for (const edge of edges) {
        if (edge.from === id) neighbors.add(edge.to);
        if (edge.to === id) neighbors.add(edge.from);
      }
      for (const [nodeId, nodeEl] of nodeEls) {
        nodeEl.classList.toggle('is-selected', nodeId === id);
        nodeEl.classList.toggle('is-dim', Boolean(id) && !neighbors.has(nodeId));
      }
      for (const edgeEl of edgeEls) {
        const hot = edgeEl.dataset.from === id || edgeEl.dataset.to === id;
        edgeEl.classList.toggle('is-hot', Boolean(id) && hot);
        edgeEl.classList.toggle('is-dim', Boolean(id) && !hot);
      }
      if (id) {
        const node = byId[id];
        for (const target of document.querySelectorAll('[data-selected-name]')) target.textContent = node.name;
        for (const target of document.querySelectorAll('[data-selected-qname]')) target.textContent = `${node.cls}.${node.name}`;
        for (const target of document.querySelectorAll('[data-selected-file]')) target.textContent = node.file;
        for (const target of document.querySelectorAll('[data-selected-kind]')) target.textContent = node.kind;
        for (const target of document.querySelectorAll('[data-fanin]')) target.textContent = node.fanIn;
        for (const target of document.querySelectorAll('[data-fanout]')) target.textContent = node.fanOut;
        for (const target of document.querySelectorAll('[data-hotspot]')) target.textContent = `${node.hotspot}`;
        if (options.onSelect) options.onSelect(node);
      }
      return nodeEls.get(id);
    }

    nodeEls.forEach((nodeEl, id) => {
      nodeEl.addEventListener('click', () => select(id));
    });
    el.addEventListener('click', (event) => {
      if (event.target.closest('.gnode')) return;
      state.selected = null;
      for (const nodeEl of nodeEls.values()) nodeEl.classList.remove('is-selected', 'is-dim');
      for (const edgeEl of edgeEls) edgeEl.classList.remove('is-hot', 'is-dim');
    });

    function fit() {
      const width = el.clientWidth;
      const height = el.clientHeight;
      if (!width || !height) return;
      const scale = Math.min(width / 1000, height / 600, fitMax);
      world.style.transform = `scale(${scale})`;
      world.style.left = `${Math.max(0, (width - 1000 * scale) / 2)}px`;
      world.style.top = `${Math.max(0, (height - 600 * scale) / 2)}px`;
    }
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    requestAnimationFrame(fit);
    if (selectedStart) select(selectedStart);
    state.api = { select, fit, observer, nodeEls };
    return state.api;
  }

  function minimap(el, layoutName = 'layered') {
    const layout = layouts[layoutName] || layouts.layered;
    el.innerHTML = '<div class="m-world"></div><div class="m-view"></div>';
    const world = el.querySelector('.m-world');
    const xs = Object.values(layout).map(([x]) => x);
    const ys = Object.values(layout).map(([, y]) => y);
    const minX = Math.min(...xs), maxX = Math.max(...xs) + 160;
    const minY = Math.min(...ys), maxY = Math.max(...ys) + 52;
    for (const node of nodes) {
      const pos = layout[node.id];
      if (!pos) continue;
      const dot = document.createElement('i');
      dot.className = 'm-node';
      dot.style.left = `${((pos[0] - minX) / (maxX - minX)) * 100}%`;
      dot.style.top = `${((pos[1] - minY) / (maxY - minY)) * 100}%`;
      dot.style.background = node.id === 'createOrder' ? 'var(--accent)' : 'var(--faint)';
      world.appendChild(dot);
    }
  }

  function fmt(n) {
    return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
  }

  window.CG = { project, nodes, edges, modules, layouts, mount, minimap, fmt, byId };
})();
