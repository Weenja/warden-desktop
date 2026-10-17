// Guarded apps on this computer. The installed apps list is read and shown locally; only the
// apps switched on are sent (set_guarded), when Done is clicked.
import { core, errorText, type PickerApp } from "./core";
import { $, appTile, applyOsWording, fitWindowToContent, followSystemTheme } from "./page";

followSystemTheme();
applyOsWording();
fitWindowToContent($("#content"));

let apps: PickerApp[] = [];
const on = new Map<string, boolean>();

function groupHead(text: string): HTMLLIElement {
  const li = document.createElement("li");
  const h = document.createElement("h2");
  h.className = "section-label picker__group";
  h.textContent = text;
  li.append(h);
  return li;
}

function row(app: PickerApp): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "app-row";
  const name = document.createElement("span");
  name.className = "app-row__name";
  name.textContent = app.name;
  const toggle = document.createElement("label");
  toggle.className = "toggle";
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = on.get(app.id) ?? false;
  input.setAttribute("aria-label", app.name);
  input.addEventListener("change", () => on.set(app.id, input.checked));
  toggle.append(input, document.createElement("span"));
  li.append(appTile(app.name, app.icon), name, toggle);
  return li;
}

function render(): void {
  const query = $<HTMLInputElement>("#search").value.trim().toLowerCase();
  const shown = apps.filter((a) => a.name.toLowerCase().includes(query));
  const common = shown.filter((a) => a.common);
  const rest = shown.filter((a) => !a.common);
  const items: HTMLLIElement[] = [];
  if (common.length > 0) items.push(groupHead("Common picks"), ...common.map(row));
  if (rest.length > 0) items.push(groupHead("All apps"), ...rest.map(row));
  if (items.length === 0) {
    const empty = document.createElement("li");
    empty.className = "picker__note";
    empty.textContent = apps.length === 0 ? "No apps found." : "No app matches.";
    items.push(empty);
  }
  $("#list").replaceChildren(...items);
}

async function load(): Promise<void> {
  apps = await core.listApps();
  for (const app of apps) on.set(app.id, app.guarded);
  render();
}

$("#search").addEventListener("input", render);
$<HTMLButtonElement>("#done").addEventListener("click", async (event) => {
  const button = event.currentTarget as HTMLButtonElement;
  const changed = apps.some((a) => (on.get(a.id) ?? false) !== a.guarded);
  if (!changed) return void core.closeWindow();
  button.disabled = true;
  $("#error").hidden = true;
  try {
    await core.saveGuardedApps(apps.filter((a) => on.get(a.id)).map((a) => ({ token: a.id, label: a.name })));
    await core.closeWindow();
  } catch (e) {
    button.disabled = false;
    $("#error").textContent = errorText(e);
    $("#error").hidden = false;
  }
});

load().catch((e) => {
  $("#list").replaceChildren(Object.assign(document.createElement("li"), { className: "picker__note", textContent: errorText(e) }));
});
