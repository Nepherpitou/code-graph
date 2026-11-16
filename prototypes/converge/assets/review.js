/* Like/dislike controls for the convergence cut.
   Votes live in localStorage; the same state is visible in the gallery and on
   every prototype page. No open text fields, no server. */
(() => {
  const PROTOS = [
    { key: 'focus', num: 'C01', title: 'Focus workbench', file: 'c01-focus-workbench.html', group: 'shell' },
    { key: 'path', num: 'C02', title: 'Path mode', file: 'c02-path-mode.html', group: 'mode' },
    { key: 'impact', num: 'C03', title: 'Impact mode', file: 'c03-impact-mode.html', group: 'mode' },
    { key: 'compare', num: 'C04', title: 'Compare pins', file: 'c04-compare-pins.html', group: 'mode' },
    { key: 'table', num: 'C05', title: 'Linked table', file: 'c05-table-drawer.html', group: 'surface' },
    { key: 'palette', num: 'C06', title: 'Command / resume', file: 'c06-command-resume.html', group: 'surface' },
    { key: 'overview', num: 'C07', title: 'Overview inspector', file: 'c07-overview-inspector.html', group: 'surface' },
    { key: 'minimal', num: 'C08', title: 'Minimal canvas', file: 'c08-minimal-canvas.html', group: 'density' },
  ];
  const KEY = 'code-graph-converge-votes-v1';
  const BY_KEY = new Map(PROTOS.map((proto) => [proto.key, proto]));
  const BY_FILE = new Map(PROTOS.map((proto) => [proto.file, proto]));
  const votes = readVotes();

  function readVotes() {
    try {
      const raw = JSON.parse(localStorage.getItem(KEY) || '{}');
      if (!raw || typeof raw !== 'object') return {};
      return Object.fromEntries(Object.entries(raw).filter(([key, value]) => (
        BY_KEY.has(key) && value && (value.vote === 'like' || value.vote === 'dislike')
      )));
    } catch {
      return {};
    }
  }

  function persist() {
    localStorage.setItem(KEY, JSON.stringify(votes));
    window.dispatchEvent(new CustomEvent('converge-votes'));
  }

  function voteOf(key) { return votes[key]?.vote || null; }
  function ratedCount() { return PROTOS.filter((proto) => voteOf(proto.key)).length; }
  function likeCount() { return PROTOS.filter((proto) => voteOf(proto.key) === 'like').length; }
  function dislikeCount() { return PROTOS.filter((proto) => voteOf(proto.key) === 'dislike').length; }

  function setVote(key, vote) {
    if (!BY_KEY.has(key)) return;
    if (vote) votes[key] = { vote, at: new Date().toISOString() };
    else delete votes[key];
    persist();
    render();
    toast(vote ? `${BY_KEY.get(key).num} · ${vote === 'like' ? 'нравится' : 'не нравится'}` : `${BY_KEY.get(key).num} · оценка убрана`);
  }

  function toast(message) {
    const node = document.createElement('div');
    node.className = 'cx-toast';
    node.textContent = message;
    document.body.appendChild(node);
    setTimeout(() => node.remove(), 1700);
  }

  function nextUnrated(afterKey) {
    const start = Math.max(0, PROTOS.findIndex((proto) => proto.key === afterKey));
    for (let step = 1; step <= PROTOS.length; step += 1) {
      const proto = PROTOS[(start + step) % PROTOS.length];
      if (!voteOf(proto.key)) return proto;
    }
    return null;
  }

  function activeProto() {
    const path = location.pathname;
    return PROTOS.find((proto) => path.endsWith(`/${proto.file}`)) || null;
  }

  /* Gallery ---------------------------------------------------------------- */
  function enhanceGallery() {
    const grid = document.querySelector('.ix-grid');
    if (!grid) return;
    for (const card of [...grid.querySelectorAll('.ix-card[href]')]) {
      const proto = BY_FILE.get(card.getAttribute('href'));
      if (!proto) continue;
      const wrap = document.createElement('div');
      wrap.className = 'ix-card-wrap';
      card.parentNode.insertBefore(wrap, card);
      wrap.appendChild(card);
      wrap.insertAdjacentHTML('beforeend', `
        <div class="ix-vote" data-vote-for="${proto.key}">
          <div class="ix-vote-buttons">
            <button class="ix-vote-btn like" data-vote="like" data-key="${proto.key}"><span>+</span>Нравится</button>
            <button class="ix-vote-btn dislike" data-vote="dislike" data-key="${proto.key}"><span>−</span>Не нравится</button>
            <button class="ix-vote-clear" data-clear="${proto.key}" title="Убрать оценку">×</button>
          </div>
          <span class="ix-vote-state" data-vote-state="${proto.key}">не оценён</span>
        </div>`);
    }
    const summary = document.createElement('div');
    summary.className = 'ix-review-summary';
    summary.innerHTML = `
      <div class="ix-review-progress"><b data-rated>0</b><span>/ ${PROTOS.length} оценено</span></div>
      <div class="ix-review-counts"><span class="like" data-likes>0</span><span class="dislike" data-dislikes>0</span></div>
      <div class="ix-review-dots">${PROTOS.map((proto) => `<a href="${proto.file}" class="ix-review-dot" data-dot="${proto.key}" title="${proto.num} · ${proto.title}">${proto.num}</a>`).join('')}</div>
      <button class="ix-vote-clear" data-reset-all title="Сбросить все оценки">сбросить</button>`;
    document.querySelector('.ix-chips')?.after(summary);
    const results = document.createElement('section');
    results.className = 'ix-results';
    results.id = 'ix-results';
    results.hidden = true;
    results.innerHTML = '<div class="ix-results-head"><div><div class="ix-kicker">Анализ · все оценены</div><h2>Результаты</h2></div><button class="ix-vote-clear" data-reset-all>сбросить</button></div><div data-results-body></div>';
    grid.after(results);
    for (const button of document.querySelectorAll('[data-vote]')) {
      button.addEventListener('click', (event) => {
        event.preventDefault();
        event.stopPropagation();
        setVote(button.dataset.key, button.dataset.vote);
      });
    }
    for (const button of document.querySelectorAll('[data-clear]')) {
      button.addEventListener('click', (event) => {
        event.preventDefault();
        event.stopPropagation();
        setVote(button.dataset.clear, null);
      });
    }
    for (const button of document.querySelectorAll('[data-reset-all]')) {
      button.addEventListener('click', () => {
        if (!confirm('Сбросить все оценки?')) return;
        for (const key of Object.keys(votes)) delete votes[key];
        persist();
        render();
      });
    }
    render();
  }

  function renderGallery() {
    const rated = ratedCount();
    for (const state of document.querySelectorAll('[data-vote-state]')) {
      const vote = voteOf(state.dataset.voteState);
      state.textContent = vote === 'like' ? 'нравится' : vote === 'dislike' ? 'не нравится' : 'не оценён';
      state.className = `ix-vote-state ${vote || ''}`;
    }
    for (const wrap of document.querySelectorAll('.ix-card-wrap')) {
      const key = wrap.querySelector('[data-vote-for]')?.dataset.voteFor;
      const vote = voteOf(key);
      wrap.classList.toggle('liked', vote === 'like');
      wrap.classList.toggle('disliked', vote === 'dislike');
      for (const button of wrap.querySelectorAll('[data-vote]')) button.classList.toggle('active', button.dataset.vote === vote);
      const clear = wrap.querySelector('[data-clear]');
      if (clear) clear.disabled = !vote;
    }
    for (const target of document.querySelectorAll('[data-rated]')) target.textContent = String(rated);
    for (const target of document.querySelectorAll('[data-likes]')) target.textContent = String(likeCount());
    for (const target of document.querySelectorAll('[data-dislikes]')) target.textContent = String(dislikeCount());
    for (const dot of document.querySelectorAll('[data-dot]')) {
      const vote = voteOf(dot.dataset.dot);
      dot.classList.toggle('like', vote === 'like');
      dot.classList.toggle('dislike', vote === 'dislike');
    }
    const results = document.getElementById('ix-results');
    if (results) {
      results.hidden = rated !== PROTOS.length;
      if (!results.hidden) renderResults(results);
    }
  }

  function renderResults(host) {
    const ranking = [...PROTOS].sort((a, b) => {
      const av = voteOf(a.key) === 'like' ? 1 : -1;
      const bv = voteOf(b.key) === 'like' ? 1 : -1;
      return bv - av || a.num.localeCompare(b.num);
    });
    const groups = {};
    for (const proto of PROTOS) {
      const group = groups[proto.group] || (groups[proto.group] = { total: 0, likes: 0 });
      group.total += 1;
      if (voteOf(proto.key) === 'like') group.likes += 1;
    }
    const top = ranking.slice(0, 4);
    const bottom = [...ranking].reverse().slice(0, 4);
    host.querySelector('[data-results-body]').innerHTML = `
      <div class="ix-results-stats">
        <div><b>${likeCount()}</b><span>нравится</span></div>
        <div><b>${dislikeCount()}</b><span>не нравится</span></div>
        <div><b>${Math.round((likeCount() / PROTOS.length) * 100)}%</b><span>approval</span></div>
        <div><b>${PROTOS.length}</b><span>оценено</span></div>
      </div>
      <div class="ix-results-cols">
        <div><h3>Сильнее</h3>${top.map((proto) => `<div class="ix-result-row"><b>${proto.num} ${proto.title}</b><span class="like">like</span></div>`).join('')}</div>
        <div><h3>Слабее</h3>${bottom.map((proto) => `<div class="ix-result-row"><b>${proto.num} ${proto.title}</b><span class="dislike">dislike</span></div>`).join('')}</div>
      </div>
      <div class="ix-results-groups">
        ${Object.entries(groups).map(([name, group]) => `<div class="ix-result-group"><span>${name}</span><b>${group.likes} / ${group.total}</b></div>`).join('')}
      </div>
      <table class="ix-ranking">
        <thead><tr><th>#</th><th>Прототип</th><th>Группа</th><th>Оценка</th></tr></thead>
        <tbody>${ranking.map((proto) => `<tr><td>${proto.num}</td><td>${proto.title}</td><td>${proto.group}</td><td class="${voteOf(proto.key) || ''}">${voteOf(proto.key) === 'like' ? 'нравится' : 'не нравится'}</td></tr>`).join('')}</tbody>
      </table>`;
  }

  /* Prototype page bar ----------------------------------------------------- */
  function enhancePage() {
    const proto = activeProto();
    if (!proto || document.querySelector('.cx-review')) return;
    document.body.classList.add('review-enabled');
    document.body.insertAdjacentHTML('beforeend', `
      <div class="cx-review" role="group" aria-label="Оценка прототипа">
        <span class="cx-review-id">${proto.num} <b>${proto.title}</b></span>
        <span class="cx-review-dots">${PROTOS.map((item) => `<a href="${item.file}" class="cx-review-dot" data-dot="${item.key}" title="${item.num} · ${item.title}">${item.num}</a>`).join('')}</span>
        <span class="cx-review-progress"><b>${ratedCount()}</b> / ${PROTOS.length}</span>
        <span class="cx-spacer"></span>
        <button class="cx-review-btn like" data-vote="like" data-key="${proto.key}"><span>+</span>Нравится</button>
        <button class="cx-review-btn dislike" data-vote="dislike" data-key="${proto.key}"><span>−</span>Не нравится</button>
        <button class="cx-review-clear" data-clear="${proto.key}" title="Убрать оценку">×</button>
        <button class="cx-review-next" data-next>Дальше</button>
        <a class="cx-review-grid" href="index.html">Все</a>
      </div>`);
    for (const button of document.querySelectorAll('.cx-review [data-vote]')) {
      button.addEventListener('click', () => setVote(button.dataset.key, button.dataset.vote));
    }
    document.querySelector('.cx-review [data-clear]')?.addEventListener('click', () => setVote(proto.key, null));
    document.querySelector('.cx-review [data-next]')?.addEventListener('click', () => {
      const next = nextUnrated(proto.key);
      location.href = next ? next.file : 'index.html#ix-results';
    });
    document.addEventListener('keydown', (event) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.target instanceof Element && event.target.closest('input, textarea, select')) return;
      const key = event.key.toLowerCase();
      if (key === 'l') { event.preventDefault(); setVote(proto.key, 'like'); }
      else if (key === 'd') { event.preventDefault(); setVote(proto.key, 'dislike'); }
      else if (key === 'u') { event.preventDefault(); setVote(proto.key, null); }
      else if (key === 'n') { event.preventDefault(); const next = nextUnrated(proto.key); location.href = next ? next.file : 'index.html#ix-results'; }
    });
    renderPage(proto);
  }

  function renderPage(proto) {
    const bar = document.querySelector('.cx-review');
    if (!bar) return;
    const vote = voteOf(proto.key);
    for (const button of bar.querySelectorAll('[data-vote]')) button.classList.toggle('active', button.dataset.vote === vote);
    const clear = bar.querySelector('[data-clear]');
    if (clear) clear.disabled = !vote;
    const progress = bar.querySelector('.cx-review-progress b');
    if (progress) progress.textContent = String(ratedCount());
    for (const dot of bar.querySelectorAll('[data-dot]')) {
      const dotVote = voteOf(dot.dataset.dot);
      dot.classList.toggle('like', dotVote === 'like');
      dot.classList.toggle('dislike', dotVote === 'dislike');
    }
    const next = nextUnrated(proto.key);
    const nextButton = bar.querySelector('[data-next]');
    if (nextButton) nextButton.textContent = next ? `Дальше · ${next.num}` : 'Результаты';
  }

  function render() {
    if (document.querySelector('.ix-grid')) renderGallery();
    const proto = activeProto();
    if (proto) renderPage(proto);
  }

  window.addEventListener('storage', () => {
    for (const key of Object.keys(votes)) delete votes[key];
    Object.assign(votes, readVotes());
    render();
  });

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
      enhanceGallery();
      enhancePage();
    });
  } else {
    enhanceGallery();
    enhancePage();
  }
})();
