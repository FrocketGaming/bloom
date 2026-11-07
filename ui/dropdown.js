// The caret dropdown: a short list of snippets next to the text cursor. It
// never takes focus, so it only draws what Bloom sends it.
const { listen } = window.__TAURI__.event;

const list = document.getElementById('list');
const previewBody = document.getElementById('prev-body');

function row(item, selected) {
  const el = document.createElement('div');
  el.className = selected ? 'row sel' : 'row';
  el.setAttribute('role', 'option');
  el.setAttribute('aria-selected', String(selected));

  const trigger = document.createElement('span');
  trigger.className = 'trig';
  trigger.textContent = item.trigger;

  const name = document.createElement('span');
  name.className = 'name';
  // A trailing ellipsis says a form will open before anything is inserted.
  name.textContent = item.form ? `${item.name}…` : item.name;

  el.append(trigger, name);
  return el;
}

function render({ rows, selected, preview }) {
  list.replaceChildren(...rows.map((item, i) => row(item, i === selected)));
  previewBody.textContent = preview;
}

listen('dropdown-rows', (event) => render(event.payload));
