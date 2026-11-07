// Bloom search palette: search snippets, fill in forms, insert.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const q = $('q');
let all = [];
let items = [];
let sel = 0;
let formSnippet = null;

const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

function subseq(needle, hay) {
  let i = 0;
  for (const c of hay) if (c === needle[i]) i++;
  return i === needle.length;
}

function search(query) {
  const qq = query.toLowerCase().trim();
  if (!qq) return all.slice();
  const bare = qq.replace(/^[^\p{L}\p{N}]+/u, '');
  const scored = [];
  for (const s of all) {
    const t = (s.trigger || '').toLowerCase();
    const n = s.name.toLowerCase();
    let score = -1;
    if (t && t.startsWith(qq)) score = 100 - t.length;
    else if (t && t.replace(/^\W+/, '').startsWith(bare)) score = 90 - t.length;
    else if (n.startsWith(bare)) score = 70;
    else if (n.includes(bare)) score = 60;
    else if (t.includes(bare)) score = 50;
    else if (s.group.toLowerCase().includes(bare)) score = 40;
    else if (s.body.toLowerCase().includes(bare)) score = 30;
    else if (subseq(bare, `${t} ${n}`)) score = 10;
    if (score >= 0) scored.push({ s, score });
  }
  return scored.sort((a, b) => b.score - a.score).map((x) => x.s);
}

function highlight(text, query) {
  const bare = query.toLowerCase().replace(/^[^\p{L}\p{N}]+/u, '').trim();
  const i = bare ? text.toLowerCase().indexOf(bare) : -1;
  if (i < 0) return esc(text);
  return esc(text.slice(0, i)) + '<mark>' + esc(text.slice(i, i + bare.length)) + '</mark>' + esc(text.slice(i + bare.length));
}

function partsHTML(parts, values) {
  return parts.map((p) => {
    switch (p.kind) {
      case 'var': return `<span class="var">${esc(p.text)}</span>`;
      case 'cursor': return '<span class="cur" title="Your cursor ends up here"></span>';
      case 'field': {
        const v = values && values[p.text];
        return v ? `<span class="field filled">${esc(v)}</span>` : `<span class="field">${esc(p.text)}</span>`;
      }
      default: return esc(p.text);
    }
  }).join('');
}

function fit() {
  requestAnimationFrame(() => {
    const h = Math.ceil($('pal').getBoundingClientRect().height);
    invoke('resize_palette', { height: h });
  });
}

// ---------- Search mode ----------

function renderList() {
  items = search(q.value);
  if (sel >= items.length) sel = Math.max(0, items.length - 1);
  const list = $('list');
  if (!all.length) {
    list.innerHTML = '<div class="empty">You have no snippets yet. Open Bloom from the tray icon to add one.</div>';
  } else if (!items.length) {
    list.innerHTML = `<div class="empty">No snippets match “${esc(q.value)}”.</div>`;
  } else {
    list.innerHTML = items.map((s, i) => `
      <div class="row${i === sel ? ' sel' : ''}" data-i="${i}" role="option" aria-selected="${i === sel}">
        ${s.trigger ? `<span class="trig">${highlight(s.trigger, q.value)}</span>` : '<span class="trig none">search</span>'}
        <span class="name">${highlight(s.name, q.value)}</span>
        ${s.fields.length ? '<span class="form-tag">form</span>' : ''}
        <span class="grp">${esc(s.group)}</span>
      </div>`).join('');
    const selEl = list.querySelector('.row.sel');
    if (selEl) selEl.scrollIntoView({ block: 'nearest' });
  }
  const s = items[sel];
  $('prev').hidden = !s;
  if (s) $('prev').innerHTML = `<div class="prev-label">Inserts</div><pre class="prev-body">${partsHTML(s.preview)}</pre>`;
  fit();
}

function move(d) {
  if (!items.length) return;
  sel = (sel + d + items.length) % items.length;
  renderList();
}

async function choose(s) {
  if (!s) return;
  try {
    const res = await invoke('insert_snippet', { id: s.id });
    if (res.status === 'needsForm') showForm(s);
  } catch (e) {
    $('list').innerHTML = `<div class="empty">${esc(String(e))}</div>`;
    fit();
  }
}

$('list').addEventListener('mousemove', (e) => {
  const row = e.target.closest('.row');
  if (row && +row.dataset.i !== sel) { sel = +row.dataset.i; renderList(); }
});
$('list').addEventListener('mousedown', (e) => {
  const row = e.target.closest('.row');
  if (row) { e.preventDefault(); choose(items[+row.dataset.i]); }
});
q.addEventListener('input', () => { sel = 0; renderList(); });
q.addEventListener('keydown', (e) => {
  if (e.key === 'ArrowDown') { e.preventDefault(); move(1); }
  else if (e.key === 'ArrowUp') { e.preventDefault(); move(-1); }
  else if (e.key === 'Enter') { e.preventDefault(); choose(items[sel]); }
});

function showSearch() {
  formSnippet = null;
  $('form-view').hidden = true;
  $('search-view').hidden = false;
  q.value = '';
  sel = 0;
  renderList();
  q.focus();
}

// ---------- Form mode ----------

function formValues() {
  const values = {};
  $('fields').querySelectorAll('[data-label]').forEach((el) => { values[el.dataset.label] = el.value; });
  return values;
}

function updateFormPreview() {
  $('f-prev').innerHTML = partsHTML(formSnippet.preview, formValues());
}

function showForm(s) {
  formSnippet = s;
  $('search-view').hidden = true;
  $('form-view').hidden = false;
  $('f-trig').textContent = s.trigger || 'search';
  $('f-trig').className = s.trigger ? 'trig' : 'trig none';
  $('f-name').textContent = s.name;
  $('f-count').textContent = s.fields.length === 1 ? '1 field' : `${s.fields.length} fields`;
  $('fields').innerHTML = s.fields.map((f, i) => {
    const id = `field-${i}`;
    const control = f.options.length
      ? `<select id="${id}" data-label="${esc(f.label)}">${f.options.map((o) => `<option>${esc(o)}</option>`).join('')}</select>`
      : `<input id="${id}" data-label="${esc(f.label)}" autocomplete="off" spellcheck="true">`;
    return `<label for="${id}">${esc(f.label)}</label>${control}`;
  }).join('');
  $('fields').querySelectorAll('[data-label]').forEach((el) => {
    el.addEventListener('input', updateFormPreview);
    el.addEventListener('change', updateFormPreview);
  });
  updateFormPreview();
  fit();
  const first = $('fields').querySelector('[data-label]');
  if (first) first.focus();
}

$('form-view').addEventListener('submit', async (e) => {
  e.preventDefault();
  if (!formSnippet) return;
  try {
    await invoke('insert_snippet', { id: formSnippet.id, values: formValues() });
  } catch (err) {
    $('f-prev').textContent = String(err);
  }
});
// Submit on Enter from any field. Browsers only do this by themselves when a
// form has exactly one text box, so snippets with several fields need it.
$('fields').addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); $('form-view').requestSubmit(); }
});

// ---------- Window ----------

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') { e.preventDefault(); invoke('hide_palette'); }
});

async function load() {
  try { all = await invoke('list_snippets'); } catch { all = []; }
}

listen('palette-open', async (e) => {
  await load();
  const formId = e.payload && e.payload.form;
  const s = formId && all.find((x) => x.id === formId);
  if (s) showForm(s); else showSearch();
});

listen('snippets-changed', async () => {
  await load();
  if (!formSnippet) renderList();
});

window.addEventListener('focus', () => {
  if (formSnippet) {
    const first = $('fields').querySelector('[data-label]');
    if (first && !$('fields').contains(document.activeElement)) first.focus();
  } else {
    q.focus();
  }
});

load().then(renderList);
