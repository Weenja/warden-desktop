// Demo only: theme toggle and state switching. [data-group] holds a state; children with [data-when="x y"] show for those states.
const root = document.documentElement;
root.dataset.theme = new URLSearchParams(location.search).get('theme') || 'dark';

function setState(group, value) {
  const box = document.querySelector(`[data-group="${group}"]`);
  box.dataset.state = value;
  box.querySelectorAll('[data-when]').forEach(el => { el.hidden = !el.dataset.when.split(' ').includes(value); });
  document.querySelectorAll(`[data-set^="${group}:"]`).forEach(b => {
    if (b.closest('.seg')) b.setAttribute('aria-pressed', b.dataset.set === `${group}:${value}`);
  });
}

document.querySelectorAll('[data-group]').forEach(g => setState(g.dataset.group, g.dataset.state));

document.addEventListener('click', e => {
  const set = e.target.closest('[data-set]');
  if (set) { const [g, v] = set.dataset.set.split(':'); setState(g, v); return; }
  const dot = e.target.closest('[data-dot]');
  if (dot) { dot.classList.toggle('is-dot'); return; }
  const sw = e.target.closest('[data-flip]');
  if (sw) sw.setAttribute('aria-checked', sw.getAttribute('aria-checked') !== 'true');
  if (e.target.closest('#theme')) root.dataset.theme = root.dataset.theme === 'light' ? 'dark' : 'light';
});
