// Demo state switching only: OS, theme, pill state, popover and dialogs.
(() => {
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const MAX = 90;
  const steam = '<span class="app-icon app-icon--%s" style="--c:#1B2838">S</span>';
  const arc = '<span class="app-icon app-icon--%s" style="--c:#FF5A5F">A</span>';
  const lock = '<svg class="ico"><use href="#i-lock"/></svg>';
  const stop = '<button class="btn btn--stop" data-action="close-app"><svg class="ico"><use href="#i-stop"/></svg>Stop</button>';
  const getExt = '<button class="btn btn--outline"><svg class="ico"><use href="#i-download"/></svg>Get the extension</button>';

  const STATES = {
    hidden: { pts: 42, face: 'idle', mood: 'Nothing draining' },
    drain: { pts: 42, face: 'idle', icon: steam, title: 'Steam · 42 pts left', sub: "−1/min while it's open", hint: 'Stop closes Steam.', mood: 'Draining' },
    low: { pts: 9, face: 'worried', icon: steam, title: 'Steam · 9 pts left', sub: 'Low battery · −1/min · MOA locks it at 0', hint: 'Stop closes Steam.', mood: 'Low battery' },
    zero: { pts: 0, face: 'locked', icon: lock, title: 'Steam · 0 pts', sub: 'MOA locks it now.', mood: 'Empty' },
    noext: { pts: 42, face: 'sideeye', icon: arc, title: "Arc · drains while it's open", sub: 'No Warden extension in Arc · −1/min', extra: getExt, mood: 'Draining', app: 'Arc' },
  };
  const tone = st => (st === 'low' ? 'low' : st === 'zero' ? 'zero' : '');

  let state = 'drain', toastShown = false, shieldTimer;

  // Fill gallery placeholders from templates
  const clone = id => document.getElementById(id).content.cloneNode(true);
  $$('.js-tpl').forEach(el => el.append(clone(el.dataset.tpl)));

  function renderPill(pill, st) {
    const s = STATES[st];
    pill.className = 'pill js-pill pill--' + st + (pill.classList.contains('is-open') ? ' is-open' : '');
    if (st === 'hidden') { pill.innerHTML = '<span class="pill__lead"></span><span></span><span class="pill__pts"></span>'; return; }
    const lvl = Math.round((s.pts / MAX) * 100);
    pill.innerHTML =
      `<span class="pill__lead">${s.icon.replace('%s', 'xs')}</span><span></span><span class="pill__pts">${s.pts} pts</span>` +
      `<div class="pill__panel"><p class="pill__title">${s.icon.replace('%s', 'sm')}${s.title}</p>` +
      `<p class="pill__sub">${s.sub}</p><div class="battery ${tone(st) && 'battery--' + tone(st)}"><div class="battery__fill" style="--level:${lvl}%"></div></div>` +
      (st === 'zero' ? '' : `<div class="pill__actions">${s.extra || ''}${stop}</div>`) +
      (s.hint ? `<p class="pill__hint">${s.hint}</p>` : '') + '</div>';
  }

  function renderPopover(root, st) {
    const s = STATES[st], lvl = Math.round((s.pts / MAX) * 100);
    $$('[data-face]', root).forEach(i => (i.hidden = i.dataset.face !== s.face));
    $('[data-bind=bal]', root).textContent = s.pts;
    $('[data-bind=state]', root).textContent = `${s.mood} · battery ${lvl}%`;
    $('[data-bind=level]', root).style.setProperty('--level', lvl + '%');
    $('[data-bind=battery]', root).className = 'battery' + (tone(st) ? ' battery--' + tone(st) : '');
    const now = $('[data-bind=now]', root);
    now.className = 'popover__now' + (st === 'hidden' ? ' popover__now--idle' : '');
    now.innerHTML = st === 'hidden' ? 'Nothing draining'
      : st === 'zero' ? `${lock}<span>Steam is locked</span>`
      : `${s.icon.replace('%s', 'sm')}<span>${s.app || 'Steam'} on this ${document.body.dataset.os === 'win' ? 'PC' : 'Mac'}</span><b>−1/min</b>`;
  }

  function setState(st) {
    state = st;
    clearTimeout(shieldTimer);
    $$('.js-demo').forEach(d => {
      renderPill($('.js-pill', d), st);
      $('.js-app', d).hidden = st === 'hidden';
      $('.js-slot-shield', d).hidden = true;
      $('.js-slot-toast', d).hidden = true;
      renderPopover(d, st);
    });
    if (st === 'zero') shieldTimer = setTimeout(() => $$('.js-slot-shield').forEach(s => (s.hidden = false)), 1100);
    if (st === 'low' && !toastShown) {
      toastShown = true;
      $$('.js-slot-toast').forEach(t => (t.hidden = false));
      setTimeout(() => $$('.js-slot-toast').forEach(t => (t.hidden = true)), 4000);
    }
    pressed('state', st);
  }

  function pressed(control, value) {
    $$(`[data-control=${control}] button`).forEach(b => b.setAttribute('aria-pressed', b.dataset.value === value));
  }

  // Demo desks: slots get their content once
  $$('.js-demo').forEach(d => {
    $('.js-slot-popover', d).append(clone('tpl-popover'));
    $('.js-slot-picker', d).append(clone('tpl-picker'));
    $('.js-slot-quit', d).append(clone('tpl-quit'));
    $('.js-slot-shield', d).append(clone('tpl-shield'));
    $('.js-slot-toast', d).append(clone('tpl-toast'));

    d.addEventListener('click', e => {
      const pill = e.target.closest('.js-pill');
      const act = e.target.closest('[data-action]')?.dataset.action;
      if (act === 'close-app') { setState('hidden'); return; }
      if (act === 'edit') { $('.js-slot-popover', d).hidden = true; $('.js-slot-picker', d).hidden = false; return; }
      if (act === 'quit') { $('.js-slot-popover', d).hidden = true; $('.js-slot-quit', d).hidden = false; return; }
      if (act === 'close') { e.target.closest('.overlay').hidden = true; return; }
      if (e.target.closest('.js-moa')) {
        const pop = $('.js-slot-popover', d);
        pop.hidden = !pop.hidden;
        e.target.closest('.js-moa').classList.toggle('is-active', !pop.hidden);
        return;
      }
      if (pill && !e.target.closest('button') && state !== 'hidden') pill.classList.toggle('is-open');
    });
  });

  // Top bar controls
  $('[data-control=os]').addEventListener('click', e => {
    const b = e.target.closest('button'); if (!b) return;
    document.body.dataset.os = b.dataset.value; pressed('os', b.dataset.value); setState(state);
  });
  $('[data-control=state]').addEventListener('click', e => {
    const b = e.target.closest('button'); if (b) setState(b.dataset.value);
  });
  $('[data-control=screen]').addEventListener('click', e => {
    const b = e.target.closest('button'); if (!b) return;
    $('#demo-mac').classList.toggle('desk--nonotch', b.dataset.value === 'nonotch'); pressed('screen', b.dataset.value);
  });
  $('[data-control=theme]').addEventListener('click', () => {
    const r = document.documentElement;
    r.dataset.theme === 'light' ? r.removeAttribute('data-theme') : (r.dataset.theme = 'light');
  });

  // Deep link for screenshots and sharing: #light, #win, #low ...
  const h = location.hash.slice(1).split(',');
  if (h.includes('light')) document.documentElement.dataset.theme = 'light';
  if (h.includes('win')) { document.body.dataset.os = 'win'; pressed('os', 'win'); }
  setState(h.find(x => STATES[x]) || 'drain');
})();
