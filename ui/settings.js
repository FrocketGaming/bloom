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

// Bloom closes this window for good (freeing its memory) when nothing is
// unsaved, and only hides it otherwise. So it has to know when something is.
let reportedUnsaved = false;
function reportUnsaved() {
  const unsaved = dirty || varsDirty;
  if (unsaved === reportedUnsaved) return;
  reportedUnsaved = unsaved;
  invoke('set_settings_unsaved', { unsaved }).catch(() => { reportedUnsaved = !unsaved; });
}

/** Save is only active when the form differs from what's saved. */
function updateDirty() {
  dirty = snapshot() !== savedSnapshot;
  reportUnsaved();
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
  refreshVariableChoices();
  $('paused').checked = settings.paused;
  $('paused-label').textContent = settings.paused ? 'Paused' : 'Expanding';
}

function renderErrors() {
  const errs = settings.errors || [];
  $('errors').hidden = errs.length === 0;
  $('errors').textContent = errs.length ? `Some things need attention:\n${errs.join('\n')}` : '';
}

// ---------- Sidebar ----------

// Groups the user folded away. Kept in the window's own storage, so it is a
// per-device convenience; if storage is unavailable, folding still works for
// the session.
const COLLAPSED_KEY = 'bloom.collapsedGroups';

function loadCollapsed() {
  try {
    const saved = JSON.parse(localStorage.getItem(COLLAPSED_KEY));
    return new Set(Array.isArray(saved) ? saved : []);
  } catch {
    return new Set();
  }
}

function saveCollapsed() {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...collapsed]));
  } catch {
    // Not saved, but the groups still fold and unfold.
  }
}

const collapsed = loadCollapsed();

function filterText() {
  return $('filter').value.toLowerCase().trim();
}

function toggleGroup(group) {
  // While filtering, every match is shown so that none can be hidden by a fold.
  if (filterText()) return;
  if (collapsed.has(group)) collapsed.delete(group); else collapsed.add(group);
  saveCollapsed();
  renderTree();
  // The list is rebuilt, so put keyboard focus back on the header just used.
  const header = [...$('tree').querySelectorAll('.group')].find((h) => h.dataset.group === group);
  if (header) header.focus();
}

function renderTree() {
  const f = filterText();
  const shown = snippets.filter((s) => !f || `${s.name} ${s.trigger || ''} ${s.body} ${s.group}`.toLowerCase().includes(f));
  // One entry per file, so a file with no snippets still has a header and can
  // be deleted. While filtering, only files with matches are listed.
  const files = (settings && settings.snippet_files) || [];
  const rows = files
    .map((file) => ({ file, items: shown.filter((s) => s.file === file.file) }))
    .filter((row) => !f || row.items.length);
  if (!rows.length) {
    $('tree').innerHTML = `<div class="empty">${snippets.length ? 'No snippets match.' : 'No snippets yet. Press New to add one.'}</div>`;
    return;
  }
  let html = '';
  for (const { file, items } of rows) {
    const group = file.group;
    const open = Boolean(f) || !collapsed.has(group);
    const count = file.snippets === 0 ? 'empty' : items.length;
    html += `<div class="group-row"><button type="button" class="group" data-group="${esc(group)}" aria-expanded="${open}"><span class="chev" aria-hidden="true"></span><span class="gname">${esc(group)}</span><span class="gcount">${count}</span></button><button type="button" class="group-del" data-file="${esc(file.file)}" title="Delete ${esc(file.file)}" aria-label="Delete ${esc(file.file)}"><svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4.5 6h11M8 6V4.5h4V6M6 6l.7 9.2c0 .5.4.8.9.8h4.8c.5 0 .9-.3.9-.8L14 6M8.4 9v4.2M11.6 9v4.2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg></button></div>`;
    if (!open) continue;
    for (const s of items) {
      const sel = current && current.id === s.id && view === 'editor' ? ' sel' : '';
      html += `<button type="button" class="item${sel}" data-id="${esc(s.id)}"><span class="nm">${esc(s.name)}</span>${s.trigger ? `<span class="trig">${esc(s.trigger)}</span>` : ''}</button>`;
    }
  }
  if (!snippets.length) html += '<div class="empty">No snippets yet. Press New to add one.</div>';
  $('tree').innerHTML = html;
}

/**
 * Moves a whole snippet file to the Recycle Bin, after asking. The dialog says
 * how many snippets it holds, so it is clear what is being removed.
 */
async function deleteFile(file) {
  const info = ((settings && settings.snippet_files) || []).find((f) => f.file === file);
  const count = info ? info.snippets : 0;
  let message = count === 0
    ? 'It has no snippets.'
    : `It holds ${count} snippet${count === 1 ? '' : 's'}.`;
  message += ' The file is moved to the Recycle Bin, where you can restore it from.';
  if (dirty && current && current.file === file) {
    message += ' Your unsaved changes to the open snippet will be lost.';
  }
  const confirmed = await askConfirm({ title: `Delete ${file}?`, message, confirm: 'Delete file', danger: true });
  if (!confirmed) return;
  try {
    await invoke('delete_snippet_file', { file });
  } catch (e) {
    await askConfirm({ title: 'The file was not deleted', message: String(e), confirm: 'OK', cancel: null });
    return;
  }
  const wasOpen = Boolean(current && current.file === file);
  if (wasOpen) { dirty = false; current = null; reportUnsaved(); }
  await loadAll();
  if (wasOpen && view === 'editor') {
    if (snippets.length) openSnippet(snippets[0]); else await newSnippet();
  }
}

$('tree').addEventListener('click', async (e) => {
  const trash = e.target.closest('.group-del');
  if (trash) { deleteFile(trash.dataset.file); return; }
  const header = e.target.closest('.group');
  if (header) { toggleGroup(header.dataset.group); return; }
  const item = e.target.closest('.item');
  if (!item) return;
  const s = snippets.find((x) => x.id === item.dataset.id);
  if (s && await confirmDiscard()) openSnippet(s);
});
$('filter').addEventListener('input', renderTree);

/**
 * Asks a yes or no question in a dialog of Bloom's own. The browser's confirm
 * box titles itself with the app's internal address ("tauri.localhost says").
 * Resolves true only if the user confirms: Esc, Cancel or a click outside is no.
 * Focus starts on the cancel button, so a stray Enter never destroys anything.
 */
function askConfirm({ title, message, confirm, cancel = 'Cancel', danger = false }) {
  const dialog = $('confirm');
  const ok = $('confirm-ok');
  $('confirm-title').textContent = title;
  $('confirm-message').textContent = message;
  // A message that only needs acknowledging has no Cancel button.
  $('confirm-cancel').hidden = cancel === null;
  if (cancel !== null) $('confirm-cancel').textContent = cancel;
  ok.textContent = confirm;
  ok.classList.toggle('danger', danger);
  ok.classList.toggle('primary', !danger);
  dialog.returnValue = '';
  return new Promise((resolve) => {
    const onClose = () => {
      dialog.removeEventListener('close', onClose);
      dialog.removeEventListener('click', onBackdrop);
      resolve(dialog.returnValue === 'ok');
    };
    // A click on the dimmed area around the dialog lands on the dialog itself.
    const onBackdrop = (e) => { if (e.target === dialog) dialog.close('cancel'); };
    dialog.addEventListener('close', onClose);
    dialog.addEventListener('click', onBackdrop);
    dialog.showModal();
  });
}

async function confirmDiscard() {
  if (view === 'variables') {
    if (!varsDirty) return true;
    return askConfirm({
      title: 'Discard your changes?',
      message: 'You have unsaved changes to your variables.',
      confirm: 'Discard',
      cancel: 'Keep editing',
      danger: true,
    });
  }
  if (!dirty) return true;
  return askConfirm({
    title: 'Discard your changes?',
    message: 'You have unsaved changes to this snippet.',
    confirm: 'Discard',
    cancel: 'Keep editing',
    danger: true,
  });
}

function showView(v) {
  view = v;
  $('editor').hidden = v !== 'editor';
  $('variables').hidden = v !== 'variables';
  $('general').hidden = v !== 'general';
  $('nav-snippets').classList.toggle('active', v === 'editor');
  $('nav-variables').classList.toggle('active', v === 'variables');
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

async function newSnippet() {
  if (!(await confirmDiscard())) return;
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
    if (s) {
      // A snippet saved into a folded group would vanish from the list.
      if (collapsed.delete(s.group)) saveCollapsed();
      openSnippet(s);
    }
    flashSaved();
  } catch (e) {
    setStatus('ed-status', String(e), 'err');
  }
}

async function remove() {
  if (!current) return;
  const confirmed = await askConfirm({
    title: 'Delete this snippet?',
    message: `“${$('ed-title').textContent}” will be removed from ${current.file}.`,
    confirm: 'Delete',
    danger: true,
  });
  if (!confirmed) return;
  try {
    await invoke('delete_snippet', { file: current.file, index: current.index });
    dirty = false;
    reportUnsaved();
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
function insertPlaceholder(ins) {
  const ta = $('ed-body');
  const start = ta.selectionStart;
  const end = ta.selectionEnd;
  ta.value = ta.value.slice(0, start) + ins + ta.value.slice(end);
  ta.focus();
  // Select the label of a new field so it can be typed over.
  if (ins.startsWith('{{field:')) ta.setSelectionRange(start + 8, start + ins.length - 2);
  else ta.setSelectionRange(start + ins.length, start + ins.length);
  updateDirty();
  updatePreview();
}

$('chips').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-ins]');
  if (b) insertPlaceholder(b.dataset.ins);
});

const MANAGE_VARIABLES = '__manage__';

/** Fills the Variable drop-down with the names currently defined. */
async function refreshVariableChoices() {
  try {
    const { variables } = await invoke('list_variables');
    $('chip-var').replaceChildren(
      el('option', { value: '', textContent: 'Variable…' }),
      ...variables.map((v) => el('option', { value: v.name, textContent: v.name })),
      el('option', { value: MANAGE_VARIABLES, textContent: variables.length ? 'Manage variables…' : 'Add a variable…' }),
    );
  } catch {
    // The drop-down keeps what it had; the buttons beside it still work.
  }
}

$('chip-var').addEventListener('change', (e) => {
  const choice = e.target.value;
  e.target.value = '';
  if (choice === MANAGE_VARIABLES) openVariables();
  else if (choice) insertPlaceholder(`{{var:${choice}}}`);
});

let previewTimer = null;
function updatePreview() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(async () => {
    const body = $('ed-body').value;
    if (!body) { $('ed-preview').innerHTML = '<span class="hint">Type the text this snippet should insert.</span>'; $('ed-fields').textContent = ''; return; }
    const { preview, fields, missing } = await invoke('preview_body', { body });
    $('ed-preview').innerHTML = preview.map((p) => {
      if (p.kind === 'var') return `<span class="var">${esc(p.text)}</span>`;
      if (p.kind === 'missing') return `<span class="missing" title="No variable has this name. It is inserted as typed.">${esc(p.text)}</span>`;
      if (p.kind === 'field') return `<span class="field">${esc(p.text)}</span>`;
      if (p.kind === 'cursor') return '<span class="cur" title="Cursor ends here"></span>';
      return esc(p.text);
    }).join('');
    const notes = [];
    if (fields.length) notes.push(`asks for ${fields.map((f) => f.label).join(', ')} before inserting`);
    if (missing.length) {
      notes.push(`${missing.map((n) => `{{var:${n}}}`).join(', ')} ${missing.length === 1 ? 'isn’t' : 'aren’t'} defined, so ${missing.length === 1 ? 'it is' : 'they are'} inserted as typed`);
    }
    $('ed-fields').textContent = notes.map((n) => `· ${n}`).join(' ');
  }, 120);
}
$('ed-body').addEventListener('input', updatePreview);

// ---------- Variables ----------

const VAR_NAME = /^[\p{L}\p{N}_-]{1,64}$/u;

let varRows = [];       // what the page is editing
let varsSaved = '';     // the rows as last loaded or saved
let varsDirty = false;
let undefinedNames = []; // used in snippets, defined nowhere

function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  Object.assign(node, props);
  node.append(...children);
  return node;
}

function rowFrom(v) {
  return { name: v.name, kind: v.kind, text: v.text, optionsText: v.options.join('\n'), usedIn: v.used_in };
}

function optionsOf(row) {
  return row.optionsText.split('\n').map((o) => o.trim()).filter(Boolean);
}

function varsSnapshot() {
  return JSON.stringify(varRows.map((r) => [r.name, r.kind, r.text, r.optionsText]));
}

function updateVarsDirty() {
  varsDirty = varsSnapshot() !== varsSaved;
  reportUnsaved();
  $('vars-unsaved').hidden = !varsDirty;
  $('vars-save').disabled = !varsDirty;
  if (varsDirty) setStatus('vars-status', '');
}

function usedText(n) {
  if (!n) return 'Not used yet';
  return n === 1 ? 'Used in 1 snippet' : `Used in ${n} snippets`;
}

function varRowEl(row) {
  const placeholder = el('code');
  const showPlaceholder = () => { placeholder.textContent = `{{var:${row.name.trim() || 'name'}}}`; };
  showPlaceholder();

  const name = el('input', { className: 'var-name', value: row.name, placeholder: 'email', autocomplete: 'off', spellcheck: false });
  name.setAttribute('aria-label', 'Variable name');
  name.addEventListener('input', () => {
    row.name = name.value;
    name.classList.remove('bad');
    showPlaceholder();
    renderUndefined();
    updateVarsDirty();
  });

  const seg = el('div', { className: 'seg', role: 'radiogroup' });
  for (const [kind, label] of [['text', 'Text'], ['choices', 'Choices']]) {
    const b = el('button', { type: 'button', textContent: label });
    b.dataset.v = kind;
    b.setAttribute('aria-checked', String(row.kind === kind));
    b.addEventListener('click', () => {
      if (row.kind === kind) return;
      row.kind = kind;
      renderVars();
      updateVarsDirty();
    });
    seg.append(b);
  }

  const isText = row.kind === 'text';
  const value = el('textarea', {
    className: 'var-value',
    value: isText ? row.text : row.optionsText,
    placeholder: isText ? 'sam@example.com' : 'Best\nCheers\nKind regards',
    spellcheck: false,
  });
  value.setAttribute('aria-label', isText ? 'Text to insert' : 'Choices, one per line');
  value.addEventListener('input', () => {
    if (row.kind === 'text') row.text = value.value; else row.optionsText = value.value;
    value.classList.remove('bad');
    updateVarsDirty();
  });

  const remove = el('button', { className: 'var-remove', type: 'button', textContent: 'Remove' });
  remove.addEventListener('click', () => {
    varRows = varRows.filter((r) => r !== row);
    renderVars();
    updateVarsDirty();
  });

  const hint = isText
    ? 'Inserted exactly as written. Placeholders inside it are not expanded.'
    : 'One choice per line. A snippet using it asks you to pick one.';
  return el('div', { className: 'var-row' },
    el('div', { className: 'var-top' }, name, seg, el('span', { className: 'var-used', textContent: usedText(row.usedIn) })),
    value,
    el('p', { className: 'hint', textContent: hint }),
    el('div', { className: 'var-foot' }, placeholder, remove));
}

function renderUndefined() {
  const box = $('vars-undefined');
  const defined = new Set(varRows.map((r) => r.name.trim().toLowerCase()));
  const open = undefinedNames.filter((u) => !defined.has(u.name.toLowerCase()));
  box.hidden = open.length === 0;
  box.replaceChildren();
  if (!open.length) return;
  box.append(el('span', { textContent: 'Used in snippets but not defined:' }));
  for (const u of open) {
    const b = el('button', { type: 'button', textContent: `${u.name} · add` });
    b.title = `${usedText(u.usedIn)}. Add a variable with this name.`;
    b.addEventListener('click', () => addVariable(u.name));
    box.append(b);
  }
}

function renderVars() {
  $('vars-list').replaceChildren(...(varRows.length
    ? varRows.map(varRowEl)
    : [el('div', { className: 'empty', textContent: 'No variables yet. Add one, then use it in a snippet as {{var:name}}.' })]));
  renderUndefined();
}

function addVariable(name = '') {
  varRows.push({ name, kind: 'text', text: '', optionsText: '', usedIn: 0 });
  renderVars();
  updateVarsDirty();
  const rows = $('vars-list').querySelectorAll('.var-row');
  const last = rows[rows.length - 1];
  if (last) last.querySelector(name ? '.var-value' : '.var-name').focus();
}

async function loadVariables() {
  const data = await invoke('list_variables');
  varRows = data.variables.map(rowFrom);
  undefinedNames = data.undefined;
  varsSaved = varsSnapshot();
  renderVars();
  updateVarsDirty();
}

/** Marks the first field with a problem and says what it is. Returns true if there was one. */
function checkVars() {
  const rowEls = $('vars-list').querySelectorAll('.var-row');
  const seen = new Set();
  const problem = (i, selector, message) => {
    const field = rowEls[i].querySelector(selector);
    field.classList.add('bad');
    field.focus();
    setStatus('vars-status', message, 'err');
    return true;
  };
  for (let i = 0; i < varRows.length; i++) {
    const r = varRows[i];
    const n = r.name.trim();
    if (!n) return problem(i, '.var-name', 'Give every variable a name.');
    if (!VAR_NAME.test(n)) return problem(i, '.var-name', `“${n}” isn’t a usable name. Use letters, digits, - and _.`);
    if (seen.has(n.toLowerCase())) return problem(i, '.var-name', `“${n}” is used twice. Names ignore case.`);
    seen.add(n.toLowerCase());
    if (r.kind === 'choices' && !optionsOf(r).length) {
      return problem(i, '.var-value', `“${n}” needs at least one choice, one per line.`);
    }
  }
  return false;
}

async function saveVars() {
  if (!varsDirty || checkVars()) return;
  const variables = varRows.map((r) => ({ name: r.name.trim(), kind: r.kind, text: r.text, options: optionsOf(r) }));
  try {
    await invoke('save_variables', { variables });
    await loadAll();
    await loadVariables();
    setStatus('vars-status', 'Saved.', 'ok');
  } catch (e) {
    setStatus('vars-status', String(e), 'err');
  }
}

async function openVariables() {
  if (!(await confirmDiscard())) return;
  dirty = false;
  reportUnsaved();
  await loadVariables();
  setStatus('vars-status', '');
  showView('variables');
}

$('nav-variables').addEventListener('click', openVariables);

/** Back to the snippet editor from Variables or Settings. */
async function openSnippets() {
  if (view === 'editor') return;
  if (!(await confirmDiscard())) return;
  dirty = false;
  varsDirty = false;
  reportUnsaved();
  // Reload the snippet from what is saved, so a form left behind by a discarded
  // edit is not shown as if it were clean.
  const again = current && snippets.find((s) => s.id === current.id);
  if (again) openSnippet(again);
  else if (snippets.length) openSnippet(snippets[0]);
  else newSnippet();
}
$('nav-snippets').addEventListener('click', openSnippets);
$('vars-add').addEventListener('click', () => addVariable());
$('vars-save').addEventListener('click', saveVars);

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
  renderStartup();
  renderAppearance();
  $('hotkey').textContent = prettyHotkey(c.hotkey);
  $('inject').querySelectorAll('button').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === c.inject)));
  $('inject-hint').textContent = INJECT_HINTS[c.inject];
  $('threshold-row').hidden = c.inject !== 'auto';
  if (document.activeElement !== $('threshold')) $('threshold').value = c.clipboard_threshold;
  $('suggestions').querySelectorAll('button').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === (c.suggestions ? 'on' : 'off'))));
  $('undo').querySelectorAll('button').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === (c.undo_with_backspace ? 'on' : 'off'))));
  $('rank').querySelectorAll('button').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === (c.rank_by_use ? 'on' : 'off'))));
  $('folder').textContent = settings.snippets_dir;
}

/** Saves a change to the settings. Returns null, or the reason it was refused. */
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
    return null;
  } catch (e) {
    renderGeneral();
    setStatus('gen-status', String(e), 'err');
    return String(e);
  }
}

// ---------- Appearance ----------

let presets = null; // the ready-made accents, asked for once

function swatch(id, label, color, selected) {
  const b = el('button', { type: 'button', className: id === 'custom' ? 'swatch custom' : 'swatch', title: label });
  b.dataset.id = id;
  b.setAttribute('role', 'radio');
  b.setAttribute('aria-label', label);
  b.setAttribute('aria-checked', String(selected));
  if (color) b.style.setProperty('--sw', color);
  return b;
}

/** `#rgb`, `#rrggbb`, with or without the `#`, as `#rrggbb`; or null. */
function normaliseHex(text) {
  const t = text.trim().replace(/^#/, '').toLowerCase();
  if (/^[0-9a-f]{3}$/.test(t)) return `#${[...t].map((c) => c + c).join('')}`;
  if (/^[0-9a-f]{6}$/.test(t)) return `#${t}`;
  return null;
}

/** Shows how a custom colour will really look once adjusted for readability. */
async function showAccentPreview(hex) {
  const out = $('accent-preview');
  try {
    const { light, dark } = await invoke('preview_accent', { color: hex });
    const chip = (c) => `<span class="chip-swatch" style="background:${c}"></span>`;
    out.innerHTML = light.accent === hex && dark.accent === hex
      ? 'Used as it is in both themes.'
      : `Shown as ${chip(light.accent)} ${light.accent} in the light theme and ${chip(dark.accent)} ${dark.accent} in the dark theme, so text stays readable.`;
  } catch (e) {
    out.textContent = '';
  }
}

async function renderAppearance() {
  const c = settings.config;
  $('theme-mode').querySelectorAll('button').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === c.theme)));
  if (!presets) {
    try { presets = (await invoke('get_theme')).presets; } catch { return; }
  }
  $('accent').replaceChildren(
    ...presets.map((p) => swatch(p.id, p.label, p.swatch, c.accent === p.id)),
    swatch('custom', 'Custom', null, c.accent === 'custom'),
  );
  const custom = c.accent === 'custom';
  $('accent-custom').hidden = !custom;
  if (custom) {
    $('accent-picker').value = c.accent_color;
    if (document.activeElement !== $('accent-hex')) $('accent-hex').value = c.accent_color;
    showAccentPreview(c.accent_color);
  }
}

$('theme-mode').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (b) saveConfig({ theme: b.dataset.v });
});
$('accent').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-id]');
  if (b) saveConfig({ accent: b.dataset.id });
});

async function saveCustomColor(hex) {
  $('accent-error').textContent = '';
  const problem = await saveConfig({ accent: 'custom', accent_color: hex });
  if (problem) $('accent-error').textContent = problem;
}
let pickTimer = null;
$('accent-picker').addEventListener('input', () => {
  // Dragging in the picker fires continuously; save once it settles.
  const hex = $('accent-picker').value;
  $('accent-hex').value = hex;
  clearTimeout(pickTimer);
  pickTimer = setTimeout(() => saveCustomColor(hex), 250);
});
$('accent-hex').addEventListener('change', () => {
  const hex = normaliseHex($('accent-hex').value);
  if (!hex) {
    $('accent-error').textContent = 'Use a hex color such as #3b82f6.';
    return;
  }
  saveCustomColor(hex);
});

// Start with Windows has three states: on, off, and not decided yet (null),
// which is what an install from before the setting existed has.
const STARTUP_HINTS = {
  on: 'Bloom starts in the tray when you sign in to Windows, so snippets work without opening it first. If it does not start, check that Bloom is switched on under Settings, Apps, Startup in Windows, which overrides this.',
  off: 'Bloom starts only when you open it. Snippets do not expand until you do.',
  undecided: 'Not decided yet: Bloom does not start with Windows until you choose.',
};

function renderStartup() {
  const choice = settings.config.start_with_windows;
  const key = choice === true ? 'on' : choice === false ? 'off' : 'undecided';
  $('startup').querySelectorAll('button').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === key)));
  $('startup-hint').textContent = STARTUP_HINTS[key];
  // The one-time question goes away as soon as it has an answer.
  $('startup-offer').hidden = key !== 'undecided';
}

$('startup').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (b) saveConfig({ start_with_windows: b.dataset.v === 'on' });
});

async function answerStartupOffer(on) {
  $('offer-error').textContent = '';
  const problem = await saveConfig({ start_with_windows: on });
  // A refusal has to be seen where the question was asked.
  if (problem) $('offer-error').textContent = problem;
}
$('offer-yes').addEventListener('click', () => answerStartupOffer(true));
$('offer-no').addEventListener('click', () => answerStartupOffer(false));

$('inject').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (b) saveConfig({ inject: b.dataset.v });
});
$('suggestions').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (b) saveConfig({ suggestions: b.dataset.v === 'on' });
});
$('undo').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (b) saveConfig({ undo_with_backspace: b.dataset.v === 'on' });
});
$('rank').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (b) saveConfig({ rank_by_use: b.dataset.v === 'on' });
});
$('threshold').addEventListener('change', () => {
  const n = parseInt($('threshold').value, 10);
  if (n > 0) saveConfig({ clipboard_threshold: n });
});
$('open-folder').addEventListener('click', () => invoke('open_snippets_folder'));

// ---------- Export and import ----------

async function exportAll() {
  setStatus('transfer-status', '');
  try {
    const path = await invoke('export_settings');
    if (path) setStatus('transfer-status', `Exported to ${path}`, 'ok');
  } catch (e) {
    setStatus('transfer-status', String(e), 'err');
  }
}

async function importAll() {
  setStatus('transfer-status', '');
  const confirmed = await askConfirm({
    title: 'Replace everything with an export?',
    message: 'Your settings, snippets, variables and most-used record are replaced by the file you pick. Bloom saves the current ones in its backups folder first.',
    confirm: 'Choose file…',
    danger: true,
  });
  if (!confirmed) return;
  try {
    const report = await invoke('import_settings');
    if (!report) return;
    // Settings, variables and the look changed underneath this window.
    await loadAll();
    const files = report.snippet_files === 1 ? '1 file' : `${report.snippet_files} files`;
    const count = report.snippets === 1 ? '1 snippet' : `${report.snippets} snippets`;
    let msg = `Imported ${count} from ${files}. Your previous setup is saved in ${report.backup}`;
    if (report.warning) msg += ` ${report.warning}`;
    setStatus('transfer-status', msg, report.warning ? 'err' : 'ok');
  } catch (e) {
    setStatus('transfer-status', String(e), 'err');
  }
}

$('export').addEventListener('click', exportAll);
$('import').addEventListener('click', importAll);
$('nav-settings').addEventListener('click', async () => { if (await confirmDiscard()) { dirty = false; varsDirty = false; reportUnsaved(); showView('general'); } });

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
  if (e.ctrlKey && e.key.toLowerCase() === 's' && view === 'variables') { e.preventDefault(); saveVars(); }
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
listen('variables-changed', async () => {
  // Keeps the warnings banner and the preview honest after variables.toml changes.
  settings = await invoke('get_settings');
  renderErrors();
  refreshVariableChoices();
  if (view === 'variables' && !varsDirty) await loadVariables();
  if (view === 'editor') updatePreview();
});
listen('paused-changed', (e) => {
  $('paused').checked = e.payload;
  $('paused-label').textContent = e.payload ? 'Paused' : 'Expanding';
});

loadAll().then(() => { if (snippets.length) openSnippet(snippets[0]); else newSnippet(); });
