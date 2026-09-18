import { invoke } from "@tauri-apps/api/core";
import {
  toDisplay,
  type Bootstrap,
  type Change,
  type Item,
  type Preview,
  type SaveResult,
  type Section,
} from "./types";
import { openCapture } from "./capture";
import {
  conflicts as findConflicts,
  risk,
  validate,
  type Conflict,
  type Entry,
  type Kind,
} from "./keys";
import { installResizer } from "./resizer";
import {
  EMPTY,
  dirtyPaths as dirtyOf,
  effective as effectiveOf,
  fromField,
  isDirty as isDirtyOf,
  label,
  newStore,
  payload as payloadOf,
  saved as savedOf,
  setEdit as setEditOf,
  stateOf as stateOfPath,
  type State,
  type Store,
} from "./state";

const el = (id: string) => document.getElementById(id)!;
const esc = (s: string) =>
  s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

let boot: Bootstrap;
let store: Store = newStore();
let filter = "";
let onlyDirty = false;
let active = 0;
let resizer: { refit: () => void } | null = null;

const saved = (p: string) => savedOf(store, p);
const effective = (p: string) => effectiveOf(store, p);
const isDirty = (p: string) => isDirtyOf(store, p);
const dirtyPaths = () => dirtyOf(store);
const stateOf = (p: string): State => stateOfPath(store, p);
const payload = () => payloadOf(store);

function allItems(): Item[] {
  return boot.schema!.sections.flatMap((s) => s.items);
}
function findItem(path: string): Item | undefined {
  return allItems().find((i) => i.path === path);
}

// --- keybindings -----------------------------------------------------------

/** Effective binding text (edits, then file, then herdr's default). */
function bindingText(item: Item): string {
  const v = effective(item.path);
  return toDisplay(v === null ? item.default : v, item.ty);
}

/** Every binding in the effective config, defaults included. */
function bindingEntries(): Entry[] {
  return allItems()
    .filter((i) => i.binding_kind)
    .map((i) => ({ path: i.path, value: bindingText(i), kind: i.binding_kind as Kind }));
}

const allConflicts = (): Conflict[] => findConflicts(bindingEntries());
const conflictsFor = (path: string) => allConflicts().filter((c) => c.paths.includes(path));

/** The prefix chord currently in effect, needed to capture `prefix+X`. */
function prefixChord(): string {
  const item = findItem("keys.prefix");
  return item ? bindingText(item) : "";
}

const RISK_LABEL = { safe: "安定", caution: "要確認", risky: "端末依存" } as const;

function keynoteFor(item: Item): string {
  if (!item.binding_kind) return "";
  const kind = item.binding_kind as Kind;
  const text = bindingText(item);
  if (!text) return `<span class="kn none">未設定</span>`;

  const parts: string[] = [];
  for (const e of validate(text, kind, item.accepts_range))
    parts.push(`<span class="kn err">${esc(e)}</span>`);
  const r = risk(text, kind);
  parts.push(`<span class="kn ${r.level}" title="${esc(r.reason)}">${RISK_LABEL[r.level]}</span>`);
  for (const c of conflictsFor(item.path)) {
    const others = c.paths.filter((p) => p !== item.path);
    parts.push(
      `<span class="kn warn">衝突: ${esc(c.chord)} → ${others.map((p) => esc(p)).join(", ")}</span>`
    );
  }
  return parts.join("");
}

async function capture(path: string) {
  const item = findItem(path);
  if (!item || !item.binding_kind) return;
  const next = await openCapture({
    path,
    title: item.path,
    kind: item.binding_kind as Kind,
    acceptsRange: item.accepts_range,
    current: bindingText(item),
    prefixChord: item.binding_kind === "prefix" ? "" : prefixChord(),
    others: bindingEntries().filter((e) => e.path !== path),
  });
  if (next === null) return; // cancelled
  setEdit(path, JSON.stringify(next));
  renderBody();
}

// --- row decorations -------------------------------------------------------

function chipFor(item: Item): string {
  if (isDirty(item.path)) return `<span class="chip dirty">未保存</span>`;
  switch (stateOf(item.path)) {
    case "inherit":
      return `<span class="chip inherit">既定</span>`;
    case "disabled":
      return `<span class="chip disabled">無効</span>`;
    default:
      return `<span class="chip set">設定済み</span>`;
  }
}

function diffFor(item: Item): string {
  if (!isDirty(item.path)) return "";
  const before = label(saved(item.path), item);
  const after = label(effective(item.path), item);
  return `<span class="from">${esc(before)}</span><span class="arrow">→</span><span class="to">${esc(
    after
  )}</span>`;
}

function actionsFor(item: Item): string {
  const st = stateOf(item.path);
  const btns: string[] = [];
  if (item.binding_kind)
    btns.push(`<button class="ghost rec" data-act="capture" data-p="${esc(item.path)}">キーを録音</button>`);
  if (st !== "inherit")
    btns.push(`<button class="ghost" data-act="reset" data-p="${esc(item.path)}">既定に戻す</button>`);
  if (item.empty_disables && st !== "disabled")
    btns.push(`<button class="ghost" data-act="disable" data-p="${esc(item.path)}">無効にする</button>`);
  return btns.join("");
}

// --- rendering -------------------------------------------------------------

function widgetFor(item: Item): string {
  const v = effective(item.path);
  const placeholder = v === EMPTY ? "無効 (空文字)" : toDisplay(item.default, item.ty) || "(未設定)";

  if (item.ty === "bool") {
    const cur = v === null ? "" : toDisplay(v, item.ty);
    return `<select data-w="${esc(item.path)}">
      <option value="" ${cur === "" ? "selected" : ""}>既定 (${esc(toDisplay(item.default, item.ty))})</option>
      <option value="true" ${cur === "true" ? "selected" : ""}>true</option>
      <option value="false" ${cur === "false" ? "selected" : ""}>false</option>
    </select>`;
  }

  const shown = v === null || v === EMPTY ? "" : toDisplay(v, item.ty);
  const type = item.ty === "integer" || item.ty === "float" ? "number" : "text";
  const listId = item.enum_candidates.length ? `dl-${item.path.replace(/\./g, "-")}` : "";
  const datalist = listId
    ? `<datalist id="${listId}">${item.enum_candidates
        .map((c) => `<option value="${esc(c)}"></option>`)
        .join("")}</datalist>`
    : "";
  const wide = item.ty === "array" || item.ty === "table" ? " wide" : "";
  return `<input type="${type}" data-w="${esc(item.path)}" value="${esc(shown)}"
    placeholder="${esc(placeholder)}" ${listId ? `list="${listId}"` : ""}
    class="field${wide}" spellcheck="false" autocomplete="off" />${datalist}`;
}

function renderItem(item: Item): string {
  const doc = [...item.doc, item.trailing].filter(Boolean).join(" ");
  const badges = [`<span class="badge">${item.ty}</span>`];
  if (item.is_key_binding) badges.push(`<span class="badge key">key</span>`);

  return `
    <div class="item ${stateOf(item.path)}${isDirty(item.path) ? " is-dirty" : ""}" data-row="${esc(item.path)}">
      <div class="bar"></div>
      <div class="body">
        <div class="item-head">
          <span class="path">${esc(item.key)}</span>
          <span class="slot-chip">${chipFor(item)}</span>
          ${widgetFor(item)}
          <span class="slot-actions">${actionsFor(item)}</span>
        </div>
        <div class="slot-diff rowdiff">${diffFor(item)}</div>
        <div class="slot-keynote keynote">${keynoteFor(item)}</div>
        ${doc ? `<div class="doc">${esc(doc)}</div>` : ""}
      </div>
    </div>`;
}

function matches(i: Item): boolean {
  if (onlyDirty && !isDirty(i.path)) return false;
  if (!filter) return true;
  const q = filter.toLowerCase();
  return (
    i.path.toLowerCase().includes(q) ||
    i.doc.join(" ").toLowerCase().includes(q) ||
    i.trailing.toLowerCase().includes(q)
  );
}

function renderBody() {
  const sections = boot.schema!.sections;
  const across = onlyDirty || !!filter;
  const list: Section[] = across ? sections : [sections[active]];
  const parts: string[] = [];

  for (const sec of list) {
    const items = sec.items.filter(matches);
    if (across && !items.length) continue;
    parts.push(`<h2>${esc(sec.name ? `[${sec.name}]` : "(root)")}</h2>`);
    if (!across && sec.doc.length) parts.push(`<div class="sec-doc">${esc(sec.doc.join(" "))}</div>`);
    parts.push(...items.map(renderItem));
    if (!across && sec.hints.length) {
      parts.push(
        `<div class="hints"><h3>設定ではない説明行（${sec.hints.length}）— enum 候補として取り込み済み</h3>${sec.hints
          .map((h) => `<div>L${h.line} ${esc(h.name)} = ${esc(h.description)}</div>`)
          .join("")}</div>`
      );
    }
  }
  if (!parts.length)
    parts.push(
      `<div class="sec-doc">${onlyDirty ? "未保存の変更はありません。" : "一致する設定がありません。"}</div>`
    );
  el("items").innerHTML = parts.join("");
  bindWidgets();
}

function renderNav() {
  const nav = el("sections");
  nav.innerHTML = "";
  boot.schema!.sections.forEach((sec, i) => {
    const set = sec.items.filter((it) => stateOf(it.path) !== "inherit").length;
    const dirty = sec.items.filter((it) => isDirty(it.path)).length;
    const b = document.createElement("button");
    b.className = i === active && !filter && !onlyDirty ? "active" : "";
    b.innerHTML = `<span>${esc(sec.name || "(root)")}</span><span class="count">${
      dirty ? `<em>${dirty}</em>` : ""
    }${set ? `<i>${set}</i>` : ""}<span class="total">${sec.items.length}</span></span>`;
    b.title = sec.name || "(root)";
    b.onclick = () => {
      active = i;
      filter = "";
      onlyDirty = false;
      (el("filter") as HTMLInputElement).value = "";
      render();
    };
    nav.appendChild(b);
  });
  resizer?.refit();
}

/** Update one row's decorations in place so the focused input is not rebuilt. */
function refreshRow(path: string) {
  const row = el("items").querySelector<HTMLElement>(`[data-row="${path}"]`);
  const item = findItem(path);
  if (!row || !item) return;
  row.className = `item ${stateOf(path)}${isDirty(path) ? " is-dirty" : ""}`;
  row.querySelector(".slot-chip")!.innerHTML = chipFor(item);
  row.querySelector(".slot-diff")!.innerHTML = diffFor(item);
  row.querySelector(".slot-actions")!.innerHTML = actionsFor(item);
  row.querySelector(".slot-keynote")!.innerHTML = keynoteFor(item);
}

function renderHeaderCounts() {
  const clashes = allConflicts();
  const cEl = el("conflicts");
  if (clashes.length) {
    cEl.className = "on";
    cEl.innerHTML = `キー衝突 <em>${clashes.length}</em>`;
    cEl.title = clashes.map((c) => `${c.chord}: ${c.paths.join(" / ")}`).join("\n");
  } else {
    cEl.className = "";
    cEl.textContent = "キー衝突 なし";
    cEl.title = "";
  }

  const n = dirtyPaths().length;
  const btn = el("only-dirty") as HTMLButtonElement;
  btn.disabled = n === 0;
  btn.classList.toggle("on", onlyDirty);
  btn.innerHTML = n ? `未保存の変更 <em>${n}</em>` : "未保存の変更 なし";
}

function renderBar(extra: string | null = null) {
  const n = dirtyPaths().length;
  const result = extra ?? (el("result")?.innerHTML || "");
  el("savebar").innerHTML = `
    <span class="dirty-count">${n ? `<em>${n}</em> 件の変更` : "変更なし"}</span>
    <button id="btn-revert" ${n ? "" : "disabled"}>すべて取り消し</button>
    <button id="btn-preview" ${n ? "" : "disabled"}>差分を確認</button>
    <button id="btn-save" class="primary" ${n ? "" : "disabled"}>保存して反映</button>
    <span id="result">${result}</span>`;

  el("btn-revert").onclick = () => {
    store.edits.clear();
    render(null);
  };
  el("btn-preview").onclick = async () => {
    const p = await invoke<Preview>("preview_edits", { edits: payload() });
    renderBar(p.error ? `<span class="bad">${esc(p.error)}</span>` : changeList(p.changes));
  };
  el("btn-save").onclick = doSave;
}

function render(result: string | null = null) {
  renderNav();
  renderBody();
  renderHeaderCounts();
  renderBar(result);
}

// --- interaction -----------------------------------------------------------

function setEdit(path: string, value: string | null) {
  setEditOf(store, path, value);
  refreshRow(path);
  renderNav();
  renderHeaderCounts();
  renderBar();
}

function bindWidgets() {
  const body = el("items");

  body.querySelectorAll<HTMLInputElement>("input[data-w]").forEach((input) => {
    const path = input.dataset.w!;
    const item = findItem(path)!;
    const commit = () => {
      try {
        // An empty field means "inherit the default": the key leaves the file.
        const next = fromField(input.value, item.ty);
        input.classList.remove("invalid");
        setEdit(path, next);
      } catch {
        input.classList.add("invalid");
      }
    };
    input.oninput = commit;
    input.onchange = commit;
  });

  body.querySelectorAll<HTMLSelectElement>("select[data-w]").forEach((sel) => {
    const path = sel.dataset.w!;
    const item = findItem(path)!;
    sel.onchange = () => setEdit(path, fromField(sel.value, item.ty));
  });

  body.onclick = (ev) => {
    const btn = (ev.target as HTMLElement).closest<HTMLButtonElement>("button[data-act]");
    if (!btn) return;
    const path = btn.dataset.p!;
    if (btn.dataset.act === "capture") {
      void capture(path);
      return;
    }
    setEdit(path, btn.dataset.act === "disable" ? EMPTY : null);
    // The field's contents change, so this row must be rebuilt.
    const row = body.querySelector<HTMLElement>(`[data-row="${path}"]`);
    const item = findItem(path)!;
    if (row) {
      row.outerHTML = renderItem(item);
      bindWidgets();
    }
  };
}

function changeList(changes: Change[]): string {
  const rows = changes
    .filter((c) => c.action !== "noop")
    .map((c) => {
      const from = c.from === null ? "(既定)" : c.from;
      const to = c.to === null ? "(既定に戻す)" : c.to;
      return `<div class="chg"><span class="act ${c.action}">${c.action}</span><span class="cp">${esc(
        c.path
      )}</span> <s>${esc(from)}</s> <span class="arrow">→</span> <b>${esc(to)}</b></div>`;
    });
  return rows.length ? `<div class="changes">${rows.join("")}</div>` : "実質的な変更はありません";
}

async function doSave() {
  let r: SaveResult;
  try {
    r = await invoke<SaveResult>("save_edits", { edits: payload(), reload: true });
  } catch (e) {
    renderBar(`<span class="bad">保存失敗: ${esc(String(e))}</span>`);
    return;
  }
  const notes = [
    `<span class="${r.check_ok ? "ok" : "bad"}">config check: ${esc(
      r.check_output || (r.check_ok ? "ok" : "failed")
    )}</span>`,
  ];
  if (r.reloaded !== null)
    notes.push(
      `<span class="${r.reloaded ? "ok" : "bad"}">reload: ${esc(
        r.reload_output || (r.reloaded ? "ok" : "failed")
      )}</span>`
    );
  if (r.backup) notes.push(`<span class="dim">backup: ${esc(r.backup)}</span>`);

  boot.config = (await invoke<Bootstrap>("bootstrap")).config;
  store = newStore(boot.config.values);
  onlyDirty = false;
  render(`${changeList(r.changes)}${notes.join(" · ")}`);
}

async function main() {
  try {
    boot = await invoke<Bootstrap>("bootstrap");
  } catch (e) {
    el("meta").innerHTML = `<span class="bad">bootstrap failed: ${esc(String(e))}</span>`;
    return;
  }

  const c = boot.config;
  const bits: string[] = [
    boot.herdr_found ? esc(boot.herdr_version ?? "herdr") : `<span class="bad">herdr が見つかりません</span>`,
  ];
  if (boot.schema) bits.push(`${boot.schema.item_count} 項目`);
  if (boot.schema_error) bits.push(`<span class="bad">${esc(boot.schema_error)}</span>`);
  bits.push(`${esc(c.path)}${c.exists ? "" : " (未作成)"}`);
  if (boot.unknown_paths.length)
    bits.push(`<span class="bad">未知 ${boot.unknown_paths.length}: ${esc(boot.unknown_paths.join(", "))}</span>`);
  if (c.parse_error) bits.push(`<span class="bad">${esc(c.parse_error)}</span>`);
  el("meta").innerHTML = bits.join(" · ");

  if (!boot.schema) return;
  store = newStore(boot.config.values);

  const f = el("filter") as HTMLInputElement;
  f.oninput = () => {
    filter = f.value.trim();
    if (filter) onlyDirty = false;
    render();
  };
  el("only-dirty").onclick = () => {
    onlyDirty = !onlyDirty;
    if (onlyDirty) {
      filter = "";
      f.value = "";
    }
    render();
  };
  render();

  resizer = installResizer({
    panel: el("sections"),
    handle: el("resizer"),
    min: 150,
    max: 560,
    storageKey: "herdr-config-gui.sidebar-width",
    labelSelector: "button > span:first-child",
    trailingSelector: ".count",
  });
}

main();
