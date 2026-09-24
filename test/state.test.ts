import assert from "node:assert/strict";
import test from "node:test";
import { clampWidth } from "../src/resizer";
import {
  addEntry,
  entryIndices,
  hasPendingWork,
  removeEntry,
} from "../src/state";
import { fromFieldFor } from "../src/state";
import { EMPTY, dirtyPaths, effective, fromField, isDirty, label, newStore, payload, setEdit, stateOf } from "../src/state";
import type { Item } from "../src/types";

const item = (over: Partial<Item> = {}): Item => ({
  line: 1,
  path: "keys.prefix",
  section: "keys",
  key: "prefix",
  ty: "string",
  default: '"ctrl+b"',
  doc: [],
  trailing: "",
  enum_candidates: [],
  optional: false,
  empty_disables: false,
  is_key_binding: true,
  binding_kind: "action",
  accepts_range: false,
  color: false,
  size: false,
  token_set: null,
  from_overlay: false,
  ...over,
});

test("an untouched setting reports the value on disk", () => {
  const st = newStore({ "keys.prefix": '"ctrl+a"' });
  assert.equal(stateOf(st, "keys.prefix"), "set");
  assert.equal(effective(st, "keys.prefix"), '"ctrl+a"');
  assert.equal(isDirty(st, "keys.prefix"), false);
});

test("a setting missing from the file inherits the default", () => {
  const st = newStore({});
  assert.equal(stateOf(st, "theme.name"), "inherit");
  assert.equal(effective(st, "theme.name"), null);
  assert.equal(label(null, item({ default: '"catppuccin"' })), '既定 (catppuccin)');
});

test("clearing the field returns the setting to its default", () => {
  const st = newStore({ "keys.prefix": '"ctrl+a"' });
  setEdit(st, "keys.prefix", fromField("", "string"));
  assert.equal(stateOf(st, "keys.prefix"), "inherit");
  assert.equal(isDirty(st, "keys.prefix"), true);
  assert.deepEqual(payload(st), [{ path: "keys.prefix", value: null, op: "set" }]);
});

test("typing a value marks the row dirty and quotes it as TOML", () => {
  const st = newStore({});
  setEdit(st, "theme.name", fromField("kanagawa", "string"));
  assert.equal(effective(st, "theme.name"), '"kanagawa"');
  assert.equal(stateOf(st, "theme.name"), "set");
  assert.deepEqual(payload(st), [{ path: "theme.name", value: '"kanagawa"', op: "set" }]);
});

test("editing back to the saved value leaves no phantom change", () => {
  const st = newStore({ "keys.prefix": '"ctrl+a"' });
  setEdit(st, "keys.prefix", fromField("ctrl+b", "string"));
  assert.equal(isDirty(st, "keys.prefix"), true);
  setEdit(st, "keys.prefix", fromField("ctrl+a", "string"));
  assert.equal(isDirty(st, "keys.prefix"), false);
  assert.deepEqual(dirtyPaths(st), []);
  assert.deepEqual(payload(st), []);
});

test("clearing an already-inherited setting is not a change", () => {
  const st = newStore({});
  setEdit(st, "theme.name", fromField("", "string"));
  assert.equal(isDirty(st, "theme.name"), false);
  assert.deepEqual(payload(st), []);
});

test("an explicit empty string is 'disabled', distinct from inherit", () => {
  const st = newStore({ "ui.window_title": '"{hostname}: {workspace}"' });
  setEdit(st, "ui.window_title", EMPTY);
  assert.equal(stateOf(st, "ui.window_title"), "disabled");
  assert.notEqual(stateOf(st, "ui.window_title"), "inherit");
  assert.equal(label(EMPTY, item({ path: "ui.window_title" })), "無効 (空文字)");
  assert.deepEqual(payload(st), [{ path: "ui.window_title", value: EMPTY, op: "set" }]);
});

test("a setting already disabled on disk is not dirty", () => {
  const st = newStore({ "ui.window_title": EMPTY });
  assert.equal(stateOf(st, "ui.window_title"), "disabled");
  assert.equal(isDirty(st, "ui.window_title"), false);
});

test("bool fields use the empty option for inherit", () => {
  const st = newStore({});
  assert.equal(fromField("", "bool"), null);
  setEdit(st, "theme.auto_switch", fromField("true", "bool"));
  assert.equal(effective(st, "theme.auto_switch"), "true");
});

test("non-numeric text in a number field is rejected, not stored", () => {
  assert.throws(() => fromField("abc", "integer"));
  assert.equal(fromField("30", "integer"), "30");
  const st = newStore({ "ui.sidebar_width": "26" });
  try {
    setEdit(st, "ui.sidebar_width", fromField("abc", "integer"));
  } catch {
    /* the UI marks the field invalid and keeps the previous state */
  }
  assert.equal(effective(st, "ui.sidebar_width"), "26");
  assert.deepEqual(payload(st), []);
});

test("arrays are taken as verbatim TOML", () => {
  const st = newStore({});
  setEdit(st, "experimental.cjk_ime_agents", fromField('["claude", "codex"]', "array"));
  assert.equal(effective(st, "experimental.cjk_ime_agents"), '["claude", "codex"]');
});

test("dirty counting spans several settings", () => {
  const st = newStore({ "keys.prefix": '"ctrl+a"', "theme.name": '"terminal"' });
  setEdit(st, "keys.prefix", null);
  setEdit(st, "theme.name", fromField("nord", "string"));
  setEdit(st, "ui.sidebar_width", fromField("30", "integer"));
  assert.deepEqual(dirtyPaths(st).sort(), ["keys.prefix", "theme.name", "ui.sidebar_width"]);
});

test("sidebar width is clamped to the allowed range", () => {
  assert.equal(clampWidth(300, 150, 560), 300);
  assert.equal(clampWidth(20, 150, 560), 150);
  assert.equal(clampWidth(9999, 150, 560), 560);
  assert.equal(clampWidth(249.6, 150, 560), 250, "sub-pixel widths are rounded");
});

// --- array-of-tables entries -----------------------------------------------

test("entries are discovered from both the file and pending edits", () => {
  const st = newStore({
    "keys.command[0].key": '"prefix+alt+g"',
    "keys.command[0].command": '"lazygit"',
    "keys.command[1].command": '"btop"',
  });
  assert.deepEqual(entryIndices(st, "keys.command"), [0, 1]);
  setEdit(st, "keys.command[2].type", '"shell"');
  assert.deepEqual(entryIndices(st, "keys.command"), [0, 1, 2]);
});

test("a new entry lands one past the highest index in use", () => {
  const st = newStore({ "keys.command[0].command": '"lazygit"' });
  assert.equal(addEntry(st, "keys.command", { type: '"shell"' }), 1);
  assert.equal(effective(st, "keys.command[1].type"), '"shell"');
  assert.equal(addEntry(st, "keys.command", { type: '"shell"' }), 2);
});

test("removing a saved entry becomes a remove_entry op", () => {
  const st = newStore({
    "keys.command[0].command": '"lazygit"',
    "keys.command[1].command": '"btop"',
  });
  removeEntry(st, "keys.command", 1);
  assert.deepEqual(entryIndices(st, "keys.command"), [0]);
  assert.deepEqual(payload(st), [
    { path: "keys.command[1]", value: null, op: "remove_entry" },
  ]);
});

test("removing an unsaved entry just drops its edits", () => {
  const st = newStore({ "keys.command[0].command": '"lazygit"' });
  const index = addEntry(st, "keys.command", { type: '"shell"' });
  setEdit(st, `keys.command[${index}].command`, '"btop"');
  removeEntry(st, "keys.command", index);
  assert.deepEqual(payload(st), [], "nothing to send: it never reached disk");
  assert.deepEqual(entryIndices(st, "keys.command"), [0]);
});

test("a removed index is not reused while the deletion is pending", () => {
  // Otherwise the new row and the deletion would fight over one index.
  const st = newStore({
    "keys.command[0].command": '"lazygit"',
    "keys.command[1].command": '"btop"',
  });
  removeEntry(st, "keys.command", 1);
  assert.equal(addEntry(st, "keys.command", { type: '"shell"' }), 2);
});

test("edits inside a removed entry are not sent", () => {
  const st = newStore({
    "keys.command[0].command": '"lazygit"',
    "keys.command[1].command": '"btop"',
  });
  setEdit(st, "keys.command[1].command", '"htop"');
  removeEntry(st, "keys.command", 1);
  assert.deepEqual(payload(st), [
    { path: "keys.command[1]", value: null, op: "remove_entry" },
  ]);
});

test("ordinary settings still travel as set ops", () => {
  const st = newStore({});
  setEdit(st, "theme.name", '"nord"');
  assert.deepEqual(payload(st), [{ path: "theme.name", value: '"nord"', op: "set" }]);
});

test("pending work covers deletions as well as edits", () => {
  const st = newStore({ "keys.command[0].command": '"lazygit"' });
  assert.equal(hasPendingWork(st), false);
  removeEntry(st, "keys.command", 0);
  assert.equal(hasPendingWork(st), true);
});

// --- per-setting field conversion ------------------------------------------

const sizeItem = () => item({ path: "keys.command[0].width", key: "width", size: true });

test("a popup dimension is quoted only when it is a percentage", () => {
  // herdr rejects `width = "120"`: a cell count must be a bare integer.
  assert.equal(fromFieldFor(sizeItem(), "80%"), '"80%"');
  assert.equal(fromFieldFor(sizeItem(), "120"), "120");
});

test("an ordinary string setting is always quoted", () => {
  assert.equal(fromFieldFor(item({ ty: "string" }), "120"), '"120"');
});

test("an empty field still means inherit, whatever the setting", () => {
  assert.equal(fromFieldFor(sizeItem(), ""), null);
  assert.equal(fromFieldFor(item({ ty: "string" }), "  "), null);
});

test("a dimension that is neither form is refused rather than quoted", () => {
  assert.throws(() => fromFieldFor(sizeItem(), "wide"));
  assert.throws(() => fromFieldFor(sizeItem(), "200%"));
});
