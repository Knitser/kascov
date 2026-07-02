/* kascov — covenant explorer for Kaspa L1 */
'use strict';

(function () {
  // ------------------------------------------------------------------ config

  var NETWORKS = ['testnet-10', 'mainnet'];
  var DEFAULT_NETWORK = 'testnet-10';
  var PAGE_SIZE = 100;
  var EXPLORER = {
    'testnet-10': 'https://explorer-tn10.kaspa.org/txs/',
    'mainnet': 'https://explorer.kaspa.org/txs/'
  };
  var KIND = {
    genesis: { label: 'genesis', cls: 'kind-genesis' },
    transition: { label: 'transition', cls: 'kind-transition' },
    burn: { label: 'burn', cls: 'kind-burn' }
  };
  var BASE_TITLE = 'kascov — Kaspa covenant explorer';

  // ------------------------------------------------------------------ state

  var state = {
    network: DEFAULT_NETWORK,
    loadState: 'loading', // loading | ready | empty | error
    data: null,
    errorMsg: '',
    query: '',
    status: 'all',
    visible: PAGE_SIZE,
    route: parseHash(),
    reqSeq: 0
  };

  var cache = new Map(); // network -> { kind: 'data', data } | { kind: 'missing' }

  // ------------------------------------------------------------------ dom

  function $(sel) { return document.querySelector(sel); }

  var el = {
    tabs: document.querySelectorAll('.network-tab'),
    chips: document.querySelectorAll('.chip'),
    panel: $('#panel'),
    viewList: $('#view-list'),
    viewDetail: $('#view-detail'),
    stats: $('#stats'),
    freshness: $('#freshness'),
    search: $('#search'),
    list: $('#cov-list'),
    listFoot: $('#list-foot'),
    count: $('#result-count')
  };

  // ------------------------------------------------------------------ utils

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }

  function abbrev(hash, n) {
    n = n || 8;
    if (hash == null || hash === '') return '—';
    var s = String(hash);
    if (s.length <= n * 2 + 1) return esc(s);
    return esc(s.slice(0, n)) + '…' + esc(s.slice(-n));
  }

  function abbrevOutpoint(outpoint) {
    var s = String(outpoint == null ? '' : outpoint);
    var i = s.lastIndexOf(':');
    if (i === -1) return abbrev(s, 10);
    return abbrev(s.slice(0, i), 10) + esc(s.slice(i));
  }

  function fmtInt(n) {
    return (typeof n === 'number' && isFinite(n)) ? n.toLocaleString('en-US') : '—';
  }

  function fmtKas(sompi, network) {
    var unit = network === 'mainnet' ? 'KAS' : 'TKAS';
    if (typeof sompi !== 'number' || !isFinite(sompi)) return '— ' + unit;
    return (sompi / 1e8).toLocaleString('en-US', {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2
    }) + ' ' + unit;
  }

  function relTime(ms) {
    if (typeof ms !== 'number' || !isFinite(ms)) return 'an unknown time';
    var s = Math.floor((Date.now() - ms) / 1000);
    if (s < 60) return 'just now';
    var m = Math.floor(s / 60);
    if (m < 60) return m + ' min ago';
    var h = Math.floor(m / 60);
    if (h < 24) return h + (h === 1 ? ' hour ago' : ' hours ago');
    var d = Math.floor(h / 24);
    return d + (d === 1 ? ' day ago' : ' days ago');
  }

  function explorerBase() {
    return EXPLORER[state.network] || EXPLORER[DEFAULT_NETWORK];
  }

  // ------------------------------------------------------------------ routing

  function parseHash() {
    var m = (location.hash || '').match(/^#\/c\/([0-9a-fA-F]{64})$/);
    if (m) return { view: 'detail', id: m[1].toLowerCase() };
    return { view: 'list' };
  }

  // ------------------------------------------------------------------ data

  function prepare(raw) {
    var data = (raw && typeof raw === 'object') ? raw : {};
    var covs = Array.isArray(data.covenants) ? data.covenants.slice() : [];
    covs.forEach(function (c) {
      c.events = Array.isArray(c.events) ? c.events.slice().sort(function (a, b) {
        return ((a && a.seq) || 0) - ((b && b.seq) || 0);
      }) : [];
      c._search = (String(c.covenant_id || '') + ' ' + c.events.map(function (e) {
        return (e && e.txid) || '';
      }).join(' ')).toLowerCase();
    });
    covs.sort(function (a, b) {
      return ((b.last_activity_daa != null ? b.last_activity_daa : -1)) -
             ((a.last_activity_daa != null ? a.last_activity_daa : -1));
    });
    data._covs = covs;
    data._byId = new Map(covs.map(function (c) {
      return [String(c.covenant_id || '').toLowerCase(), c];
    }));
    return data;
  }

  function loadNetwork(net) {
    var seq = ++state.reqSeq;
    var cached = cache.get(net);
    if (cached) { applyLoaded(cached); return; }

    state.loadState = 'loading';
    render();

    fetch('data/' + net + '.json', { cache: 'no-store' })
      .then(function (res) {
        if (res.status === 404) return { kind: 'missing' };
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json().then(function (json) {
          return { kind: 'data', data: prepare(json) };
        });
      })
      .then(function (entry) {
        cache.set(net, entry);
        if (seq !== state.reqSeq || net !== state.network) return;
        applyLoaded(entry);
      })
      .catch(function (err) {
        if (seq !== state.reqSeq || net !== state.network) return;
        state.loadState = 'error';
        state.errorMsg = 'Could not load the ' + net + ' snapshot (' +
          (err && err.message ? err.message : 'network error') + ').';
        render();
      });
  }

  function applyLoaded(entry) {
    if (entry.kind === 'missing' || !entry.data._covs.length) {
      state.data = entry.kind === 'data' ? entry.data : null;
      state.loadState = 'empty';
    } else {
      state.data = entry.data;
      state.loadState = 'ready';
    }
    render();
  }

  // ------------------------------------------------------------------ render

  function render() {
    var detail = state.route.view === 'detail';

    if (state.loadState !== 'ready') {
      el.viewList.hidden = true;
      el.viewDetail.hidden = true;
      el.panel.hidden = false;
      document.title = BASE_TITLE;
      if (state.loadState === 'loading') {
        el.panel.innerHTML =
          '<div class="state-panel">' +
            '<div class="spinner" aria-hidden="true"></div>' +
            '<p>loading ' + esc(state.network) + ' snapshot…</p>' +
          '</div>';
      } else if (state.loadState === 'error') {
        el.panel.innerHTML =
          '<div class="state-panel">' +
            '<div class="state-glyph" aria-hidden="true">⚠</div>' +
            '<h2>snapshot unavailable</h2>' +
            '<p>' + esc(state.errorMsg) + '</p>' +
            '<button type="button" class="btn" data-retry>retry</button>' +
          '</div>';
      } else { // empty
        var second = state.network === 'mainnet'
          ? 'Mainnet covenant traffic hasn’t started yet; the complete history begins with whoever indexes from day one.'
          : 'No ' + esc(state.network) + ' covenant traffic has been indexed yet; the complete history begins with whoever indexes from day one.';
        el.panel.innerHTML =
          '<div class="state-panel">' +
            '<div class="state-glyph" aria-hidden="true">◇</div>' +
            '<h2>0 covenants indexed — kascov is watching.</h2>' +
            '<p>' + second + '</p>' +
          '</div>';
      }
      return;
    }

    el.panel.hidden = true;
    el.panel.innerHTML = '';
    el.viewList.hidden = detail;
    el.viewDetail.hidden = !detail;
    if (detail) renderDetail();
    else { document.title = BASE_TITLE; renderList(); }
  }

  // ---------- list view ----------

  function filteredCovs() {
    var covs = state.data._covs;
    if (state.status !== 'all') {
      covs = covs.filter(function (c) { return c.status === state.status; });
    }
    var q = state.query.trim().toLowerCase();
    if (q) {
      covs = covs.filter(function (c) { return c._search.indexOf(q) !== -1; });
    }
    return covs;
  }

  function statCard(value, key) {
    return '<div class="stat-card"><div class="stat-v">' + value +
           '</div><div class="stat-k">' + key + '</div></div>';
  }

  function rowHtml(c) {
    var st = c.status === 'burned' ? 'burned' : 'active';
    var lineage = c.lineage_complete
      ? '<span class="lineage ok" title="Complete lineage from genesis">✓ lineage</span>'
      : '<span class="lineage warn" title="Earlier events were pruned before indexing began">⚠ truncated</span>';
    return '<a class="cov-row" href="#/c/' + esc(c.covenant_id) + '">' +
      '<div class="cov-row-head">' +
        '<span class="mono cov-id" title="' + esc(c.covenant_id) + '">' + abbrev(c.covenant_id) + '</span>' +
        '<span class="badge status-' + st + '">' + st + '</span>' +
        lineage +
      '</div>' +
      '<div class="cov-row-meta">' +
        '<span class="meta-item"><span class="meta-k">events</span><span class="meta-v">' + fmtInt(c.event_count) + '</span></span>' +
        '<span class="meta-item"><span class="meta-k">live UTXOs</span><span class="meta-v">' + fmtInt(c.live_utxos) + '</span></span>' +
        '<span class="meta-item"><span class="meta-k">value</span><span class="meta-v">' + fmtKas(c.live_value, state.network) + '</span></span>' +
        '<span class="meta-item"><span class="meta-k">last DAA</span><span class="meta-v">' + fmtInt(c.last_activity_daa) + '</span></span>' +
      '</div>' +
    '</a>';
  }

  function renderList() {
    var data = state.data;
    var stats = data.stats || {};

    el.stats.innerHTML =
      statCard(fmtInt(stats.covenants), 'covenants') +
      statCard(fmtInt(stats.active), 'active') +
      statCard(fmtInt(stats.burned), 'burned') +
      statCard(fmtInt(stats.events), 'events') +
      statCard(fmtKas(stats.live_value, state.network), 'live value');

    el.freshness.textContent =
      'snapshot from ' + relTime(data.generated_at_ms) +
      (stats.last_activity_daa != null ? ' · last activity at DAA ' + fmtInt(stats.last_activity_daa) : '');
    el.freshness.title = isFinite(data.generated_at_ms)
      ? new Date(data.generated_at_ms).toLocaleString()
      : '';

    var covs = filteredCovs();
    var shown = covs.slice(0, state.visible);

    if (!covs.length) {
      el.list.innerHTML =
        '<div class="inline-empty">' +
          '<p>no covenants match</p>' +
          '<p class="dim">try a different covenant id, event txid, or status filter</p>' +
        '</div>';
      el.listFoot.innerHTML = '';
    } else {
      el.list.innerHTML = shown.map(rowHtml).join('');
      var remaining = covs.length - shown.length;
      el.listFoot.innerHTML = remaining > 0
        ? '<button type="button" class="btn" data-more>show ' +
            fmtInt(Math.min(PAGE_SIZE, remaining)) + ' more · ' +
            fmtInt(remaining) + ' remaining</button>'
        : '';
    }

    var label = fmtInt(covs.length) + (covs.length === 1 ? ' covenant' : ' covenants');
    if (covs.length !== data._covs.length) label += ' of ' + fmtInt(data._covs.length);
    if (shown.length < covs.length) label = 'showing ' + fmtInt(shown.length) + ' of ' + label;
    el.count.textContent = label;
  }

  // ---------- detail view ----------

  function timelineHtml(c) {
    var base = explorerBase();
    var items = [];

    if (!c.lineage_complete || c.genesis_txid == null) {
      items.push(
        '<li class="tl-note"><span class="tl-dot" aria-hidden="true"></span>' +
        '<p>lineage truncated — earlier events were pruned before kascov began indexing</p></li>'
      );
    }

    c.events.forEach(function (ev) {
      var kind = KIND[ev.kind] || { label: esc(String(ev.kind || 'event')), cls: '' };
      items.push(
        '<li class="tl-event ' + kind.cls + '">' +
          '<span class="tl-dot" aria-hidden="true"></span>' +
          '<div class="tl-head">' +
            '<span class="tl-seq mono">#' + fmtInt(ev.seq) + '</span>' +
            '<span class="tl-kind">' + kind.label + '</span>' +
            '<span class="tl-daa">DAA ' + fmtInt(ev.accepting_daa) + '</span>' +
          '</div>' +
          '<div class="tl-tx">' +
            '<span class="tl-label">tx</span>' +
            (ev.txid
              ? '<a class="mono tx-link" href="' + base + esc(ev.txid) +
                '" target="_blank" rel="noopener noreferrer" title="' + esc(ev.txid) + '">' +
                abbrev(ev.txid, 12) +
                '<svg aria-hidden="true" viewBox="0 0 12 12" width="11" height="11"><path d="M4 2h6v6M10 2 3.5 8.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg></a>'
              : '<span class="mono tx-link">—</span>') +
          '</div>' +
          '<div class="tl-block mono" title="' + esc(ev.accepting_block || '') + '">block ' +
            abbrev(ev.accepting_block, 8) + '</div>' +
        '</li>'
      );
    });

    if (!c.events.length) {
      items.push(
        '<li class="tl-note"><span class="tl-dot" aria-hidden="true"></span>' +
        '<p>no events captured in this snapshot</p></li>'
      );
    }

    if (c.events_truncated) {
      items.push(
        '<li class="tl-note"><span class="tl-dot" aria-hidden="true"></span>' +
        '<p>event list truncated — showing ' + fmtInt(c.events.length) +
        ' of ' + fmtInt(c.event_count) + ' events</p></li>'
      );
    }

    return '<ol class="timeline">' + items.join('') + '</ol>';
  }

  function utxoCard(u) {
    var asm = Array.isArray(u.script_asm) ? u.script_asm : [];
    return '<div class="utxo-card">' +
      '<div class="utxo-head">' +
        '<span class="mono outpoint" title="' + esc(u.outpoint || '') + '">' + abbrevOutpoint(u.outpoint) + '</span>' +
        '<span class="badge ' + (u.live ? 'status-active' : 'status-spent') + '">' + (u.live ? 'live' : 'spent') + '</span>' +
        '<span class="utxo-value">' + fmtKas(u.value, state.network) + '</span>' +
      '</div>' +
      '<div class="utxo-meta">' +
        '<span>created at DAA ' + fmtInt(u.created_daa) + '</span>' +
        (u.uses_covenant_ops ? '<span class="badge op-badge">covenant ops</span>' : '') +
        (u.uses_zk_ops ? '<span class="badge op-badge zk">zk ops</span>' : '') +
      '</div>' +
      (asm.length
        ? '<details class="script"><summary>script · ' + asm.length +
          (asm.length === 1 ? ' op' : ' ops') + '</summary>' +
          '<pre class="script-body">' + asm.map(esc).join('\n') + '</pre></details>'
        : '') +
    '</div>';
  }

  function renderDetail() {
    var c = state.data._byId.get(state.route.id);

    if (!c) {
      document.title = BASE_TITLE;
      el.viewDetail.innerHTML =
        '<a class="back-link" href="#/">← back to covenants</a>' +
        '<div class="state-panel">' +
          '<div class="state-glyph" aria-hidden="true">◇</div>' +
          '<h2>covenant not found</h2>' +
          '<p>No covenant with id <span class="mono">' + abbrev(state.route.id) +
          '</span> exists in the current ' + esc(state.network) + ' snapshot.</p>' +
        '</div>';
      return;
    }

    document.title = abbrev(c.covenant_id) + ' · kascov';

    var st = c.status === 'burned' ? 'burned' : 'active';
    var lineage = c.lineage_complete
      ? '<span class="lineage ok" title="Complete lineage from genesis">✓ complete lineage</span>'
      : '<span class="lineage warn" title="Earlier events were pruned before indexing began">⚠ lineage truncated</span>';
    var utxos = Array.isArray(c.utxos) ? c.utxos : [];

    el.viewDetail.innerHTML =
      '<a class="back-link" href="#/">← back to covenants</a>' +
      '<div class="detail-head">' +
        '<h2>covenant <span class="badge status-' + st + '">' + st + '</span> ' + lineage + '</h2>' +
        '<div class="full-id-wrap">' +
          '<code class="full-id">' + esc(c.covenant_id) + '</code>' +
          '<button type="button" class="icon-btn" data-copy="' + esc(c.covenant_id) + '" aria-label="Copy covenant id to clipboard">' +
            '<svg aria-hidden="true" viewBox="0 0 14 14" width="13" height="13"><rect x="4.5" y="4.5" width="8" height="8" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M9.5 4.5v-2a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2" fill="none" stroke="currentColor" stroke-width="1.4"/></svg>' +
            '<span>copy</span>' +
          '</button>' +
        '</div>' +
      '</div>' +
      '<div class="detail-stats">' +
        statCard(fmtInt(c.event_count), 'events') +
        statCard(fmtInt(c.live_utxos), 'live UTXOs') +
        statCard(fmtKas(c.live_value, state.network), 'live value') +
        statCard(fmtInt(c.genesis_daa), 'genesis DAA') +
        statCard(fmtInt(c.last_activity_daa), 'last activity DAA') +
      '</div>' +
      '<section class="detail-section" aria-label="Lineage timeline">' +
        '<h3>lineage</h3>' + timelineHtml(c) +
      '</section>' +
      '<section class="detail-section" aria-label="UTXOs">' +
        '<h3>UTXOs <span class="dim">(' + fmtInt(utxos.length) + ')</span></h3>' +
        '<div class="utxo-list">' +
          (utxos.length
            ? utxos.map(utxoCard).join('')
            : '<p class="inline-empty small">no UTXOs tracked for this covenant' +
              (st === 'burned' ? ' — it has been burned' : '') + '</p>') +
        '</div>' +
      '</section>';
  }

  // ------------------------------------------------------------------ clipboard

  function copyText(text, btn) {
    function done(ok) {
      var label = btn.querySelector('span');
      if (!label) return;
      label.textContent = ok ? 'copied' : 'copy failed';
      btn.classList.toggle('copied', ok);
      window.setTimeout(function () {
        label.textContent = 'copy';
        btn.classList.remove('copied');
      }, 1400);
    }

    function fallback() {
      var ok = false;
      try {
        var ta = document.createElement('textarea');
        ta.value = text;
        ta.setAttribute('readonly', '');
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        ok = document.execCommand('copy');
        ta.remove();
      } catch (e) { ok = false; }
      done(ok);
    }

    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(
        function () { done(true); },
        fallback
      );
    } else {
      fallback();
    }
  }

  // ------------------------------------------------------------------ events

  el.tabs.forEach(function (tab) {
    tab.addEventListener('click', function () {
      var net = tab.getAttribute('data-network');
      if (NETWORKS.indexOf(net) === -1 || net === state.network) return;
      state.network = net;
      state.visible = PAGE_SIZE;
      el.tabs.forEach(function (t) {
        t.setAttribute('aria-pressed', String(t === tab));
      });
      if (state.route.view === 'detail') {
        location.hash = '#/'; // hashchange updates route + rerenders
      }
      loadNetwork(net);
    });
  });

  el.chips.forEach(function (chip) {
    chip.addEventListener('click', function () {
      var status = chip.getAttribute('data-status');
      if (status === state.status) return;
      state.status = status;
      state.visible = PAGE_SIZE;
      el.chips.forEach(function (ch) {
        ch.setAttribute('aria-pressed', String(ch === chip));
      });
      if (state.loadState === 'ready' && state.route.view === 'list') renderList();
    });
  });

  var searchTimer = null;
  el.search.addEventListener('input', function () {
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(function () {
      state.query = el.search.value;
      state.visible = PAGE_SIZE;
      if (state.loadState === 'ready' && state.route.view === 'list') renderList();
    }, 120);
  });

  el.listFoot.addEventListener('click', function (e) {
    if (!e.target.closest('[data-more]')) return;
    state.visible += PAGE_SIZE;
    renderList();
  });

  el.panel.addEventListener('click', function (e) {
    if (!e.target.closest('[data-retry]')) return;
    cache.delete(state.network);
    loadNetwork(state.network);
  });

  el.viewDetail.addEventListener('click', function (e) {
    var btn = e.target.closest('[data-copy]');
    if (btn) copyText(btn.getAttribute('data-copy'), btn);
  });

  window.addEventListener('hashchange', function () {
    var prev = state.route.view;
    state.route = parseHash();
    if (state.route.view !== prev) window.scrollTo(0, 0);
    render();
  });

  // ------------------------------------------------------------------ init

  loadNetwork(state.network);
})();
