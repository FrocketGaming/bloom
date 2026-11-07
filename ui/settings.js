// Bloom settings window: browse, create, edit and delete snippets; change settings.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

let snippets = [];
let settings = null;
let current = null; // { id, file, index } of the snippet being edited, or null for a new one
let view = 'editor';
let dirty = false;
let savedSnapshot = '';
let savedFlashTimer = null;
const NEW_FILE = '__new__';

// ---------- Unsaved-changes tracking ----------

function snapshot() {
  return JSON.stringify([
    $('ed-name').value, $('ed-trigger').value, $('ed-body').value, $('ed-word').checked,
    current ? current.file : chosenFile(),
  ]);
}

/** Call after loading or saving: the form now matches what's on disk. */
function markClean() {
  savedSnapshot = snapshot();
  updateDirty();
}

/** Save is only active when the form differs from what's saved. */
function updateDirty() {
  dirty = snapshot() !== savedSnapshot;
  const btn = $('save');
  if (!btn.classList.contains('saved')) btn.disabled = !dirty;
  $('ed-unsaved').hidden = !dirty;
}

function flashSaved() {
  const btn = $('save');
  clearTimeout(savedFlashTimer);
  btn.classList.add('saved');
  btn.disabled = true;
  btn.textContent = '✓ Saved';
  savedFlashTimer = setTimeout(() => {
    btn.classList.remove('saved');
    btn.textContent = 'Save';
    updateDirty();
  }, 1600);
}

// ---------- Loading ----------

async function loadAll() {
  [snippets, settings] = await Promise.all([invoke('list_snippets'), invoke('get_settings')]);
  renderErrors();
  renderTree();
  renderGeneral();
  $('paused').checked = settings.paused;
  $('paused-label').textContent = settings.paused ? 'Paused' : 'Expanding';
}

function renderErrors() {
  const errs = settings.errors || [];
  $('errors').hidden = errs.length === 0;
  $('errors').textContent = errs.length ? `Some things need attention:\n${errs.join('\n')}` : '';
}

// ---------- Sidebar ----------

function renderTree() {
  const f = $('filter').value.toLowerCase().trim();
  const shown = snippets.filter((s) => !f || `${s.name} ${s.trigger || ''} ${s.body} ${s.group}`.toLowerCase().includes(f));
  if (!shown.length) {
    $('tree').innerHTML = `<div class="empty">${snippets.length ? 'No snippets match.' : 'No snippets yet. Press New to add one.'}</div>`;
    return;
  }
  let html = '';
  let group = null;
  for (const s of shown) {
    if (s.group !== group) { group = s.group; html += `<div class="group">${esc(group)}</div>`; }
    const sel = current && current.id === s.id && view === 'editor' ? ' sel' : '';
    html += `<button type="button" class="item${sel}" data-id="${esc(s.id)}"><span class="nm">${esc(s.name)}</span>${s.trigger ? `<span class="trig">${esc(s.trigger)}</span>` : ''}</button>`;
  }
  $('tree').innerHTML = html;
}

$('tree').addEventListener('click', (e) => {
  const item = e.target.closest('.item');
  if (!item) return;
  const s = snippets.find((x) => x.id === item.dataset.id);
  if (s && confirmDiscard()) openSnippet(s);
});
$('filter').addEventListener('input', renderTree);

function confirmDiscard() {
  return !dirty || window.confirm('You have unsaved changes to this snippet. Discard them?');
}

function showView(v) {
  view = v;
  $('editor').hidden = v !== 'editor';
  $('general').hidden = v !== 'general';
  $('nav-settings').classList.toggle('active', v === 'general');
  renderTree();
}

// ---------- Editor ----------

function fillFileSelect(selected) {
  restoreFileSelect();
  const files = settings.files.length ? settings.files.slice() : ['personal.toml'];
  if (selected && !files.includes(selected)) files.push(selected);
  $('ed-file').innerHTML = files.map((f) => `<option value="${esc(f)}">${esc(f)}</option>`).join('')
    + `<option value="${NEW_FILE}">New file…</option>`;
  $('ed-file').value = selected || files[0];
}

function openSnippet(s) {
  current = { id: s.id, file: s.file, index: s.index };
  $('ed-title').textContent = s.name;
  $('ed-name').value = s.name === s.trigger ? '' : s.name;
  $('ed-trigger').value = s.trigger || '';
  $('ed-body').value = s.body;
  $('ed-word').checked = s.word;
  fillFileSelect(s.file);
  $('ed-file').disabled = true; // moving between files isn't supported yet
  $('delete').hidden = false;
  setStatus('ed-status', '');
  markClean();
  showView('editor');
  updatePreview();
}

function newSnippet() {
  if (!confirmDiscard()) return;
  current = null;
  $('ed-title').textContent = 'New snippet';
  $('ed-name').value = '';
  $('ed-trigger').value = '';
  $('ed-body').value = '';
  $('ed-word').checked = true;
  fillFileSelect(null);
  $('ed-file').disabled = false;
  $('delete').hidden = true;
  setStatus('ed-status', '');
  markClean();
  showView('editor');
  updatePreview();
  $('ed-name').focus();
}

function onFileChange() {
  if ($('ed-file').value !== NEW_FILE) return;
  // Swap the select for a text box to name the new file.
  const input = document.createElement('input');
  input.id = 'ed-newfile';
  input.placeholder = 'work.toml';
  input.autocomplete = 'off';
  $('ed-file').replaceWith(input);
  input.addEventListener('input', updateDirty);
  input.focus();
}
$('ed-file').addEventListener('change', onFileChange);

function chosenFile() {
  const nf = $('ed-newfile');
  if (nf) return nf.value.trim();
  return $('ed-file').value;
}

function restoreFileSelect() {
  const nf = $('ed-newfile');
  if (!nf) return;
  const sel = document.createElement('select');
  sel.id = 'ed-file';
  nf.replaceWith(sel);
  sel.addEventListener('change', onFileChange);
}

async function save() {
  if (!dirty) return;
  const edit = {
    file: current ? current.file : chosenFile(),
    index: current ? current.index : null,
    trigger: $('ed-trigger').value.trim() || null,
    name: $('ed-name').value.trim() || null,
    body: $('ed-body').value,
    word: $('ed-word').checked,
  };
  if (!edit.file) { setStatus('ed-status', 'Name the new file, for example work.toml.', 'err'); return; }
  try {
    const id = await invoke('save_snippet', { edit });
    restoreFileSelect();
    await loadAll();
    const s = snippets.find((x) => x.id === id);
    if (s) openSnippet(s);
    flashSaved();
  } catch (e) {
    setStatus('ed-status', String(e), 'err');
  }
}

async function remove() {
  if (!current) return;
  if (!window.confirm(`Delete “${$('ed-title').textContent}”? This removes it from ${current.file}.`)) return;
  try {
    await invoke('delete_snippet', { file: current.file, index: current.index });
    dirty = false;
    await loadAll();
    if (snippets.length) openSnippet(snippets[0]); else newSnippet();
    setStatus('ed-status', 'Deleted.', 'ok');
  } catch (e) {
    setStatus('ed-status', String(e), 'err');
  }
}

$('save').addEventListener('click', save);
$('delete').addEventListener('click', remove);
$('new').addEventListener('click', newSnippet);

for (const id of ['ed-name', 'ed-trigger', 'ed-body', 'ed-word']) {
  $(id).addEventListener('input', () => { updateDirty(); setStatus('ed-status', ''); });
}
$('editor').addEventListener('change', (e) => { if (e.target.id === 'ed-file') updateDirty(); });
$('ed-name').addEventListener('input', () => {
  $('ed-title').textContent = $('ed-name').value.trim() || $('ed-trigger').value.trim() || (current ? 'Untitled' : 'New snippet');
});

// Insert placeholder buttons
$('chips').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-ins]');
  if (!b) return;
  const ta = $('ed-body');
  const start = ta.selectionStart;
  const end = ta.selectionEnd;
  const ins = b.dataset.ins;
  ta.value = ta.value.slice(0, start) + ins + ta.value.slice(end);
  ta.focus();
  // Select the label of a new field so it can be typed over.
  if (ins.startsWith('{{field:')) ta.setSelectionRange(start + 8, start + ins.length - 2);
  else ta.setSelectionRange(start + ins.length, start + ins.length);
  updateDirty();
  updatePreview();
});

let previewTimer = null;
function updatePreview() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(async () => {
    const body = $('ed-body').value;
    if (!body) { $('ed-preview').innerHTML = '<span class="hint">Type the text this snippet should insert.</span>'; $('ed-fields').textContent = ''; return; }
    const { preview, fields } = await invoke('preview_body', { body });
    $('ed-preview').innerHTML = preview.map((p) => {
      if (p.kind === 'var') return `<span class="var">${esc(p.text)}</span>`;
      if (p.kind === 'field') return `<span class="field">${esc(p.text)}</span>`;
      if (p.kind === 'cursor') return '<span class="cur" title="Cursor ends here"></span>';
      return esc(p.text);
    }).join('');
    $('ed-fields').textContent = fields.length
      ? `· asks for ${fields.map((f) => f.label).join(', ')} before inserting`
      : '';
  }, 120);
}
$('ed-body').addEventListener('input', updatePreview);

// ---------- General settings ----------

const INJECT_HINTS = {
  auto: 'Short text is typed. Long or multi-line text is pasted, and your clipboard is restored afterwards.',
  keys: 'Always types text as keystrokes. Works everywhere, but slower for long snippets. Multi-line text presses Enter, which sends messages in some chat apps.',
  clipboard: 'Always pastes. Fast, and your clipboard is restored afterwards.',
};

function prettyHotkey(h) {
  return h.split('+').map((p) => p.replace(/^Key([A-Z])$/, '$1').replace(/^Digit(\d)$/, '$1')).join(' + ');
}

function renderGeneral() {
  const c = settings.config;
  $('hotkey').textContent = prettyHotkey(c.hotkey);
  $('inject').querySelectorAll('button').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === c.inject)));
  $('inject-hint').textContent = INJECT_HINTS[c.inject];
  $('threshold-row').hidden = c.inject !== 'auto';
  if (document.activeElement !== $('threshold')) $('threshold').value = c.clipboard_threshold;
  $('suggestions').querySelectorAll('button').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === (c.suggestions ? 'on' : 'off'))));
  $('folder').textContent = settings.snippets_dir;
}

async function saveConfig(patch) {
  const config = { ...settings.config, ...patch };
  try {
    await invoke('save_settings', { config });
    settings.config = config;
    renderGeneral();
    setStatus('gen-status', 'Saved.', 'ok');
    const errs = await invoke('get_settings');
    settings.errors = errs.errors;
    renderErrors();
  } catch (e) {
    renderGeneral();
    setStatus('gen-status', String(e), 'err');
  }
}

$('inject').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (b) saveConfig({ inject: b.dataset.v });
});
$('suggestions').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (b) saveConfig({ suggestions: b.dataset.v === 'on' });
});
$('threshold').addEventListener('change', () => {
  const n = parseInt($('threshold').value, 10);
  if (n > 0) saveConfig({ clipboard_threshold: n });
});
$('open-folder').addEventListener('click', () => invoke('open_snippets_folder'));
$('nav-settings').addEventListener('click', () => { if (confirmDiscard()) { dirty = false; showView('general'); } });

// Hotkey recorder
let recording = false;
$('hotkey').addEventListener('click', () => {
  recording = true;
  $('hotkey').classList.add('rec');
  $('hotkey').textContent = 'Press a combination…';
  $('hotkey-hint').textContent = 'Use at least one of Ctrl, Alt or Shift. Esc cancels.';
});
$('hotkey').addEventListener('blur', stopRecording);
function stopRecording() {
  if (!recording) return;
  recording = false;
  $('hotkey').classList.remove('rec');
  $('hotkey-hint').textContent = 'Click to change.';
  renderGeneral();
}
$('hotkey').addEventListener('keydown', (e) => {
  if (!recording) return;
  e.preventDefault();
  if (e.key === 'Escape') { stopRecording(); return; }
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
  const mods = [];
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (e.metaKey) mods.push('Super');
  if (!mods.length || (mods.length === 1 && mods[0] === 'Shift')) {
    $('hotkey-hint').textContent = 'Add Ctrl or Alt so the hotkey doesn’t get in the way of normal typing.';
    return;
  }
  const key = e.code.replace(/^Key/, '').replace(/^Digit/, '');
  stopRecording();
  saveConfig({ hotkey: [...mods, key].join('+') });
});

// ---------- Pause, shortcuts, events ----------

$('paused').addEventListener('change', () => {
  const p = $('paused').checked;
  $('paused-label').textContent = p ? 'Paused' : 'Expanding';
  invoke('set_paused', { paused: p });
});

document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.key.toLowerCase() === 's' && view === 'editor') { e.preventDefault(); save(); }
  if (e.ctrlKey && e.key.toLowerCase() === 'n') { e.preventDefault(); newSnippet(); }
});

function setStatus(id, msg, kind) {
  $(id).textContent = msg;
  $(id).className = 'status' + (kind ? ' ' + kind : '');
}

listen('snippets-changed', async () => {
  await loadAll();
  // Keep the editor in sync with outside edits unless the user is mid-edit.
  if (current && !dirty) {
    const s = snippets.find((x) => x.id === current.id);
    if (s) openSnippet(s);
  }
});
listen('paused-changed', (e) => {
  $('paused').checked = e.payload;
  $('paused-label').textContent = e.payload ? 'Paused' : 'Expanding';
});

loadAll().then(() => { if (snippets.length) openSnippet(snippets[0]); else newSnippet(); });
