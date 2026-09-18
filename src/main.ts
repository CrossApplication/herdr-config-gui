import { invoke } from "@tauri-apps/api/core";
import {
  isFatal,
  sizeToDisplay,
  sizeToToml,
  toDisplay,
  type Bootstrap,
  type CheckReport,
  type Change,
  type Item,
  type Preview,
  type SaveResult,
  type Section,
} from "./types";
import { FORM_LABEL, colorForm, colorToHex } from "./color";
import { openCapture } from "./capture";
import { openProblems, problemSummary, type ProblemsHost } from "./problems";
import {
  conflicts as findConflicts,
  risk,
  validate,
  type BindingInfo,
  type Conflict,
  type Entry,
  type Kind,
} from "./keys";
import { installResizer } from "./resizer";
import {
  EMPTY,
  addEntry,
  dirtyPaths as dirtyOf,
  entryIndices,
  removeEntry,
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
/** Edits plus entry deletions: everything a save would send. */
const pendingCount = () => dirtyOf(store).length + store.removedEntries.size;

function allItems(): Item[] {
  return boot.schema!.sections.flatMap((s) => s.items);
}
/**
 * Items in an array-of-tables section are templates: the schema knows
 * `keys.command.key`, while the document holds `keys.command[0].key`. Looking
 * up an indexed path returns the template rewritten to that path, so
 * everything downstream (state, diffs, capture) works off `item.path` as
 * usual.
 */
function findItem(path: string): Item | undefined {
  const direct = allItems().find((i) => i.path === path);
  if (direct) return direct;
  const template = allItems().find((i) => i.path === path.replace(/\[\d+\]/g, ""));
  return template ? { ...template, path } : undefined;
}

/** Every item the form can actually edit, with entry templates expanded. */
function allEditableItems(): Item[] {
  const out: Item[] = [];
  for (const sec of boot.schema!.sections) {
    if (!sec.array_of_tables) {
      out.push(...sec.items);
      continue;
    }
    for (const index of entryIndices(store, sec.name)) {
      for (const tpl of sec.items) out.push({ ...tpl, path: `${sec.name}[${index}].${tpl.key}` });
    }
  }
  return out;
}

// --- keybindings -----------------------------------------------------------

/** Effective binding text (edits, then file, then herdr's default). */
function bindingText(item: Item): string {
  const v = effective(item.path);
  return toDisplay(v === null ? item.default : v, item.ty);
}

/** Every binding in the effective config, defaults included. */
function bindingEntries(): Entry[] {
  return allEditableItems()
    .filter((i) => i.binding_kind)
    .map((i) => ({ path: i.path, value: bindingText(i), kind: i.binding_kind as Kind }));
}

/** Same as bindingEntries, plus the range flag the validator needs. */
function bindingInfos(): BindingInfo[] {
  return allEditableItems()
    .filter((i) => i.binding_kind)
    .map((i) => ({
      path: i.path,
      value: bindingText(i),
      kind: i.binding_kind as Kind,
      acceptsRange: i.accepts_range,
    }));
}

const allConflicts = (): Conflict[] => findConflicts(bindingEntries());
const conflictsFor = (path: string) => allConflicts().filter((c) => c.paths.includes(path));

/** The prefix chord currently in effect, needed to capture `prefix+X`. */
function prefixChord(): string {
  const item = findItem("keys.prefix");
  return item ? bindingText(item) : "";
}

const RISK_LABEL = { safe: "安定", caution: "要確認", risky: "端末依存" } as const;

function colorNote(item: Item): string {
  if (!item.color) return "";
  const v = effective(item.path);
  if (v === null) return `<span class="kn none">テーマ既定</span>`;
  const text = toDisplay(v, item.ty);
  const form = colorForm(text);
  const cls = form === "malformed" ? "err" : form === "empty" ? "none" : "safe";
  const swatch =
    colorToHex(text) !== null
      ? `<span class="kn-chip" style="background:${colorToHex(text)}"></span>`
      : "";
  return `${swatch}<span class="kn ${cls}">${FORM_LABEL[form]}</span>`;
}

function keynoteFor(item: Item): string {
  if (item.color) return colorNote(item);
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

/** Jump to a setting's row from anywhere and make it obvious which one it is. */
function focusRow(path: string) {
  const idx = boot.schema!.sections.findIndex((s) => s.items.some((i) => i.path === path));
  if (idx < 0) return;
  active = idx;
  filter = "";
  onlyDirty = false;
  (el("filter") as HTMLInputElement).value = "";
  render();
  const row = el("items").querySelector<HTMLElement>(`[data-row="${path}"]`);
  if (!row) return;
  row.scrollIntoView({ block: "center", behavior: "smooth" });
  row.classList.add("flash");
  setTimeout(() => row.classList.remove("flash"), 1600);
}

function stateLabel(path: string): string {
  if (isDirty(path)) return "未保存";
  switch (stateOf(path)) {
    case "inherit":
      return "既定";
    case "disabled":
      return "無効";
    default:
      return "設定済み";
  }
}

const problemsHost = (): ProblemsHost => ({
  bindings: bindingInfos,
  valueOf: (path) => {
    const item = findItem(path);
    return item ? bindingText(item) : "";
  },
  stateLabel,
  onFocus: focusRow,
  onCapture: capture,
  onDisable: (path) => {
    setEdit(path, EMPTY);
    renderBody();
  },
});

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

  const shown =
    v === null || v === EMPTY ? "" : item.size ? sizeToDisplay(v) : toDisplay(v, item.ty);

  if (item.color) {
    const hex = colorToHex(shown);
    // The picker always writes #rrggbb; names, rgb() and "reset" stay typeable.
    return `<input type="color" class="swatch" data-sw="${esc(item.path)}"
        value="${hex ?? "#000000"}" title="色を選ぶ（#rrggbb で書き込みます）" />
      <input type="text" data-w="${esc(item.path)}" value="${esc(shown)}"
        placeholder="${esc(item.default ? toDisplay(item.default, item.ty) : "テーマ既定")}"
        class="field" spellcheck="false" autocomplete="off" />`;
  }

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
  if (item.from_overlay)
    badges.push(
      `<span class="badge ov" title="herdr --default-config には載っていませんが、herdr が受け付ける設定です">未文書</span>`
    );

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

/** A short label for an entry card, taken from whatever identifies it. */
function entryLabel(section: string, index: number): string {
  for (const key of ["description", "command", "key"]) {
    const v = effective(`${section}[${index}].${key}`);
    if (v !== null && v !== EMPTY) return toDisplay(v, "string");
  }
  return "(未入力)";
}

/**
 * An array-of-tables section is a list, not a set of settings: its schema
 * items describe the shape of one entry. Writing them as a plain table makes
 * herdr discard the whole config, so they are only ever rendered per entry.
 */
function renderEntries(sec: Section): string {
  const indices = entryIndices(store, sec.name);
  const cards = indices.map((index) => {
    const fields = sec.items
      .map((tpl) => findItem(`${sec.name}[${index}].${tpl.key}`))
      .filter((i): i is Item => i !== undefined)
      .map(renderItem)
      .join("");
    return `<div class="entry">
        <div class="entry-head">
          <span class="entry-no">#${index + 1}</span>
          <span class="entry-label">${esc(entryLabel(sec.name, index))}</span>
          <span class="cap-spacer"></span>
          <button class="ghost danger" data-del-entry="${esc(sec.name)}" data-idx="${index}">
            このエントリを削除
          </button>
        </div>
        ${fields}
      </div>`;
  });

  const empty = indices.length
    ? ""
    : `<div class="sec-doc">エントリがありません。追加すると <code>[[${esc(sec.name)}]]</code> として書き込まれます。</div>`;
  return `${empty}${cards.join("")}
    <button class="ghost add" data-add-entry="${esc(sec.name)}">エントリを追加</button>`;
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
    // Filtering or the dirty-only view cannot slice a list of entries apart.
    if (across && sec.array_of_tables) continue;
    const items = sec.items.filter(matches);
    if (across && !items.length) continue;
    const header = sec.array_of_tables ? `[[${sec.name}]]` : sec.name ? `[${sec.name}]` : "(root)";
    parts.push(`<h2>${esc(header)}</h2>`);
    if (!across && sec.doc.length) parts.push(`<div class="sec-doc">${esc(sec.doc.join(" "))}</div>`);
    if (sec.array_of_tables) {
      parts.push(renderEntries(sec));
    } else {
      parts.push(...items.map(renderItem));
    }
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
  const { total, errors } = problemSummary(problemsHost());
  const pEl = el("problems") as HTMLButtonElement;
  pEl.className = errors ? "has-error" : total ? "has-warn" : "";
  pEl.innerHTML = total
    ? `キー設定の問題 <em>${total}</em>`
    : "キー設定の問題 なし";
  pEl.title = total ? "クリックで内容と直し方を表示" : "問題は見つかっていません";

  const n = pendingCount();
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
    store.removedEntries.clear();
    render(null);
  };
  el("btn-preview").onclick = async () => {
    const p = await invoke<Preview>("check_edits", { edits: payload() });
    renderBar(
      p.error
        ? `<span class="bad">${esc(p.error)}</span>`
        : changeList(p.changes) + checkReport(p.check)
    );
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

  body.querySelectorAll<HTMLInputElement>("input[data-sw]").forEach((sw) => {
    const path = sw.dataset.sw!;
    sw.oninput = () => {
      setEdit(path, JSON.stringify(sw.value));
      const field = body.querySelector<HTMLInputElement>(`input[data-w="${path}"]`);
      if (field) field.value = sw.value;
    };
  });

  body.querySelectorAll<HTMLSelectElement>("select[data-w]").forEach((sel) => {
    const path = sel.dataset.w!;
    const item = findItem(path)!;
    sel.onchange = () => setEdit(path, fromField(sel.value, item.ty));
  });

  body.onclick = (ev) => {
    const target = ev.target as HTMLElement;

    const add = target.closest<HTMLButtonElement>("button[data-add-entry]");
    if (add) {
      const section = add.dataset.addEntry!;
      // Seeded with a valid type so the new row is a real entry rather than a
      // blank one; herdr warns about the missing command until it is filled.
      addEntry(store, section, { type: '"shell"' });
      render();
      return;
    }
    const del = target.closest<HTMLButtonElement>("button[data-del-entry]");
    if (del) {
      removeEntry(store, del.dataset.delEntry!, Number(del.dataset.idx));
      render();
      return;
    }

    const btn = target.closest<HTMLButtonElement>("button[data-act]");
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

const DIAG_LABEL: Record<string, string> = {
  unknown_section: "未知のセクション",
  unknown_key: "未知のキー",
  type: "型が不正",
  variant: "値が不正",
  syntax: "構文エラー",
  other: "その他",
};

/** What herdr said about the candidate config, in the user's terms. */
function checkReport(c: CheckReport | null): string {
  if (!c) return "";
  if (c.unavailable) return `<div class="ck note">herdr で検証できませんでした: ${esc(c.unavailable)}</div>`;
  if (c.ok) return `<div class="ck ok">herdr config check: 問題なし</div>`;

  const rows = c.diagnostics.map((d) => {
    const bits: string[] = [
      `<span class="ck-badge ${d.severity}">${DIAG_LABEL[d.kind] ?? d.kind}</span>`,
    ];
    if (d.path) bits.push(`<code>${esc(d.path)}</code>`);
    if (d.line) bits.push(`<span class="ck-line">${d.line} 行目</span>`);
    if (d.expected) bits.push(`期待される型: <code>${esc(d.expected)}</code>`);
    if (d.allowed.length)
      bits.push(`使える値: ${d.allowed.map((a) => `<code>${esc(a)}</code>`).join(" / ")}`);
    bits.push(`<span class="ck-msg">${esc(d.message)}</span>`);
    return `<div class="ck-row ${d.severity}">${bits.join(" ")}</div>`;
  });

  const head = c.discards_config
    ? `<div class="ck bad">この内容では herdr が<b>設定ファイル全体を破棄して既定値に戻します</b>。保存はできません。</div>`
    : `<div class="ck warn">herdr が無視する項目があります（他の設定は有効です）。</div>`;
  return head + `<div class="ck-rows">${rows.join("")}</div>`;
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
  // The pre-flight check refused it: nothing was written, so keep the edits.
  if (!r.written && isFatal(r.check)) {
    renderBar(
      `<div class="ck bad">保存を中止しました。編集内容はそのまま残っています。</div>` +
        checkReport(r.check) +
        changeList(r.changes)
    );
    return;
  }

  const notes: string[] = [];
  if (r.reloaded !== null)
    notes.push(
      `<span class="${r.reloaded ? "ok" : "bad"}">reload: ${esc(
        r.reload_output || (r.reloaded ? "ok" : "失敗")
      )}</span>`
    );
  if (r.backup) notes.push(`<span class="dim">backup: ${esc(r.backup)}</span>`);
  if (!r.written) notes.push(`<span class="dim">${esc(r.reload_output || "書き込みなし")}</span>`);

  boot.config = (await invoke<Bootstrap>("bootstrap")).config;
  store = newStore(boot.config.values);
  onlyDirty = false;
  render(`${changeList(r.changes)}${checkReport(r.check)}${notes.join(" · ")}`);
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
  el("problems").onclick = () => openProblems(problemsHost());
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
