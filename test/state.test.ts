import assert from "node:assert/strict";
import test from "node:test";
import { clampWidth } from "../src/resizer";
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
  assert.deepEqual(payload(st), [{ path: "keys.prefix", value: null }]);
});

test("typing a value marks the row dirty and quotes it as TOML", () => {
  const st = newStore({});
  setEdit(st, "theme.name", fromField("kanagawa", "string"));
  assert.equal(effective(st, "theme.name"), '"kanagawa"');
  assert.equal(stateOf(st, "theme.name"), "set");
  assert.deepEqual(payload(st), [{ path: "theme.name", value: '"kanagawa"' }]);
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
  assert.deepEqual(payload(st), [{ path: "ui.window_title", value: EMPTY }]);
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
