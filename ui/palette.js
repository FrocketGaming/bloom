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

/** Lower-cases the text searches look at, once per load rather than once per
 *  snippet on every keystroke. */
function prepare(list) {
  for (const s of list) {
    s.lcTrigger = (s.trigger || '').toLowerCase();
    s.lcBare = s.lcTrigger.replace(/^\W+/, '');
    s.lcName = s.name.toLowerCase();
    s.lcGroup = s.group.toLowerCase();
    s.lcBody = s.body.toLowerCase();
    s.lcTriggerName = `${s.lcTrigger} ${s.lcName}`;
  }
  return list;
}

function search(query) {
  const qq = query.toLowerCase().trim();
  // Nothing typed yet: the snippets used most come first, and the rest keep
  // their order (the sort is stable).
  if (!qq) return all.slice().sort((a, b) => b.usage - a.usage);
  const bare = qq.replace(/^[^\p{L}\p{N}]+/u, '');
  const scored = [];
  for (const s of all) {
    const t = s.lcTrigger;
    let score = -1;
    if (t && t.startsWith(qq)) score = 100 - t.length;
    else if (t && s.lcBare.startsWith(bare)) score = 90 - t.length;
    else if (s.lcName.startsWith(bare)) score = 70;
    else if (s.lcName.includes(bare)) score = 60;
    else if (t.includes(bare)) score = 50;
    else if (s.lcGroup.includes(bare)) score = 40;
    else if (s.lcBody.includes(bare)) score = 30;
    else if (subseq(bare, s.lcTriggerName)) score = 10;
    if (score >= 0) scored.push({ s, score });
  }
  // How well it matches decides first; use only breaks ties between equally
  // good matches, so a favourite never outranks a better match.
  return scored.sort((a, b) => b.score - a.score || b.s.usage - a.s.usage).map((x) => x.s);
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
      case 'missing': return `<span class="missing" title="No variable has this name">${esc(p.text)}</span>`;
      case 'cursor': return '<span class="cur" title="Your cursor ends up here"></span>';
      case 'field': {
        const v = values && values[p.text];
        return v ? `<span class="field filled">${esc(v)}</span>` : `<span class="field">${esc(p.text)}</span>`;
      }
      default: return esc(p.text);
    }
  }).join('');
}

let lastHeight = 0;
function fit() {
  requestAnimationFrame(() => {
    // Measure the content, not the box: once the palette reaches the screen
    // height it scrolls, and its box stops tracking what it holds.
    // scrollHeight rounds to a whole pixel, and a form's rows can add up to a
    // fraction more than that. The window is then a hair too short and an
    // outer scrollbar appears, so round the real height up instead.
    const content = Array.from($('pal').children)
      .reduce((sum, el) => sum + el.getBoundingClientRect().height, 0);
    const h = Math.ceil(Math.max($('pal').scrollHeight, content));
    // Resizing the window can re-enter here; only ask the backend when the
    // height actually changed.
    if (h === lastHeight) return;
    lastHeight = h;
    invoke('resize_palette', { height: h });
  });
}

// ---------- Search mode ----------

// Rows are added to the page a batch at a time as the list is scrolled, so a
// large library never means thousands of rows to build on every keystroke.
const BATCH = 50;
let rendered = 0;

function rowHTML(s, i) {
  return `
      <div class="row" data-i="${i}" role="option" aria-selected="false">
        ${s.trigger ? `<span class="trig">${highlight(s.trigger, q.value)}</span>` : '<span class="trig none">search</span>'}
        <span class="name">${highlight(s.name, q.value)}</span>
        ${s.fields.length ? '<span class="form-tag">form</span>' : ''}
        <span class="grp">${esc(s.group)}</span>
      </div>`;
}

/** Adds rows until row `upTo` (at least one more batch) is on the page. */
function renderMore(upTo) {
  const end = Math.min(items.length, Math.max(upTo + 1, rendered + BATCH));
  if (end <= rendered) return;
  let html = '';
  for (let i = rendered; i < end; i++) html += rowHTML(items[i], i);
  $('list').insertAdjacentHTML('beforeend', html);
  rendered = end;
}

function renderList() {
  items = search(q.value);
  if (sel >= items.length) sel = Math.max(0, items.length - 1);
  const list = $('list');
  rendered = 0;
  selectedRow = null;
  if (!all.length) {
    list.innerHTML = '<div class="empty">You have no snippets yet. Open Bloom from the tray icon to add one.</div>';
  } else if (!items.length) {
    list.innerHTML = `<div class="empty">No snippets match “${esc(q.value)}”.</div>`;
  } else {
    list.innerHTML = '';
    renderMore(sel);
  }
  selectRow(sel, true);
}

$('list').addEventListener('scroll', () => {
  const list = $('list');
  if (rendered < items.length && list.scrollTop + list.clientHeight >= list.scrollHeight - 80) {
    renderMore(rendered);
  }
});

let selectedRow = null;
let previewToken = 0;

/** The preview of a snippet, asked for once and kept until the list is next
 *  loaded. Previews are built on demand because building one for every snippet
 *  would read the clipboard and the clock for all of them. */
async function previewOf(s) {
  if (!s.preview) {
    try { s.preview = await invoke('preview_snippet', { id: s.id }); } catch { s.preview = []; }
  }
  return s.preview;
}

/** Moves the highlight without rebuilding the list: hovering used to redraw
 *  every row each time the pointer crossed one. Only the old and the new row
 *  are touched. */
function selectRow(i, scroll) {
  sel = i;
  if (i >= rendered) renderMore(i);
  const row = items.length ? $('list').children[i] : null;
  if (selectedRow && selectedRow !== row) {
    selectedRow.classList.remove('sel');
    selectedRow.setAttribute('aria-selected', 'false');
  }
  selectedRow = row;
  if (row) {
    row.classList.add('sel');
    row.setAttribute('aria-selected', 'true');
    if (scroll) row.scrollIntoView({ block: 'nearest' });
  }
  const s = items[sel];
  $('prev').hidden = !s;
  if (s) {
    const token = ++previewToken;
    const show = (parts) => {
      $('prev').innerHTML = `<div class="prev-label">Inserts</div><pre class="prev-body">${partsHTML(parts)}</pre>`;
      // A longer preview makes the content taller, and the window has to follow
      // or the whole panel grows a second scrollbar beside the list's.
      fit();
    };
    if (s.preview) show(s.preview);
    else previewOf(s).then((parts) => { if (token === previewToken) show(parts); });
  }
  fit();
}

function move(d) {
  if (!items.length) return;
  selectRow((sel + d + items.length) % items.length, true);
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
  if (row && +row.dataset.i !== sel) selectRow(+row.dataset.i, false);
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

async function showForm(s) {
  await previewOf(s);
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
  try { all = prepare(await invoke('list_snippets')); } catch { all = []; }
}

listen('palette-open', async (e) => {
  await load();
  const formId = e.payload && e.payload.form;
  const s = formId && all.find((x) => x.id === formId);
  if (s) await showForm(s); else showSearch();
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
