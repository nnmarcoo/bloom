/* per-page searchable lists (formats, modifiers) */
function wireListFilter(root) {
  const input = root.querySelector('.fmt-search');
  const list = root.querySelector('.fmt-list');
  const empty = root.querySelector('.fmt-empty');
  if (!input || !list) return;
  const rows = Array.from(list.querySelectorAll('.fmt'));
  const word = /modifier/i.test(input.placeholder) ? 'modifier' : 'format';
  const noMatch = word === 'modifier'
    ? raw => 'No modifier matches “' + raw + '”.'
    : raw => '“' + raw + '” isn’t supported.';
  input.addEventListener('input', () => {
    const raw = input.value.trim();
    const q = raw.toLowerCase().replace(/\./g, '');
    list.classList.toggle('searching', q.length > 0);
    const words = q.split(/\s+/).filter(Boolean);
    let any = false;
    rows.forEach(r => {
      const keys = r.dataset.keys.split(' ');
      const match = !words.length || words.every(w => keys.some(t => t.startsWith(w)));
      r.classList.toggle('hidden', !match);
      any = any || match;
    });
    if (empty) {
      empty.hidden = any;
      empty.textContent = any ? '' : noMatch(raw);
    }
  });
}

document.querySelectorAll('.docsection:has(.fmt-search)').forEach(wireListFilter);

/* global docs search over the prebuilt index */
(function () {
  const form = document.querySelector('.guide-search');
  if (!form) return;
  const input = form.querySelector('.guide-search-input');
  const results = form.querySelector('.guide-search-results');
  let index = null;
  let items = [];
  let active = -1;

  function load() {
    if (index) return index;
    index = fetch('search-index.json')
      .then(r => r.json())
      .catch(() => []);
    return index;
  }

  function render(matches, q) {
    results.innerHTML = '';
    active = -1;
    if (!q) { results.hidden = true; return; }
    if (!matches.length) {
      const none = document.createElement('div');
      none.className = 'guide-search-none';
      none.textContent = 'No matches for “' + q + '”.';
      results.appendChild(none);
      results.hidden = false;
      return;
    }
    matches.forEach(m => {
      const a = document.createElement('a');
      a.className = 'guide-search-hit';
      a.href = m.url;
      a.innerHTML = '<span class="guide-search-hit-title">' + m.title +
        '</span><span class="guide-search-hit-sec">' + m.section + '</span>';
      results.appendChild(a);
    });
    results.hidden = false;
  }

  function search(q) {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    return items
      .map(it => {
        const hay = (it.title + ' ' + it.text).toLowerCase();
        let score = 0;
        for (const w of words) {
          if (it.title.toLowerCase().includes(w)) score += 3;
          else if (hay.includes(w)) score += 1;
          else return null;
        }
        return { it, score };
      })
      .filter(Boolean)
      .sort((a, b) => b.score - a.score)
      .slice(0, 8)
      .map(x => x.it);
  }

  function run() {
    const q = input.value.trim();
    load().then(data => {
      items = data;
      render(q ? search(q) : [], q);
    });
  }

  input.addEventListener('focus', load);
  input.addEventListener('input', run);
  form.addEventListener('submit', e => e.preventDefault());

  input.addEventListener('keydown', e => {
    const hits = Array.from(results.querySelectorAll('.guide-search-hit'));
    if (e.key === 'ArrowDown' && hits.length) {
      e.preventDefault();
      active = (active + 1) % hits.length;
    } else if (e.key === 'ArrowUp' && hits.length) {
      e.preventDefault();
      active = (active - 1 + hits.length) % hits.length;
    } else if (e.key === 'Enter' && hits.length) {
      e.preventDefault();
      (hits[active] || hits[0]).click();
      return;
    } else if (e.key === 'Escape') {
      input.value = '';
      results.hidden = true;
      input.blur();
      return;
    } else {
      return;
    }
    hits.forEach((h, i) => h.classList.toggle('active', i === active));
  });

  document.addEventListener('click', e => {
    if (!form.contains(e.target)) results.hidden = true;
  });
})();
