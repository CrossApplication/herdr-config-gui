import assert from "node:assert/strict";
import test from "node:test";
import {
  canonical,
  conflicts,
  expand,
  fromEvent,
  parse,
  risk,
  scopeOf,
  validate,
  withPrefix,
  type Entry,
  type KeyEventLike,
  type Kind,
} from "../src/keys";
import fixture from "./fixtures/default-bindings.json";

const ev = (over: Partial<KeyEventLike>): KeyEventLike => ({
  key: "a",
  code: "KeyA",
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  ...over,
});

// --- capture ---------------------------------------------------------------

test("a plain chord is captured as herdr syntax", () => {
  assert.equal(fromEvent(ev({ key: "b", code: "KeyB", ctrlKey: true })), "ctrl+b");
});

test("only modifiers held means keep waiting", () => {
  for (const k of ["Control", "Shift", "Alt", "Meta"]) {
    assert.equal(fromEvent(ev({ key: k, code: `${k}Left` })), null, k);
  }
});

test("shift does not leak into the letter", () => {
  // The browser reports an uppercase key; herdr wants the letter plus shift.
  assert.equal(fromEvent(ev({ key: "R", code: "KeyR", ctrlKey: true, shiftKey: true })), "ctrl+shift+r");
});

test("alt on macOS composes the character, so the physical code wins", () => {
  // Alt+a reports "å" as ev.key on a US macOS layout.
  assert.equal(fromEvent(ev({ key: "å", code: "KeyA", altKey: true }), "mac"), "alt+a");
});

test("cmd is spelled cmd on macOS and super elsewhere", () => {
  const e = ev({ key: "k", code: "KeyK", metaKey: true });
  assert.equal(fromEvent(e, "mac"), "cmd+k");
  assert.equal(fromEvent(e, "other"), "super+k");
});

test("named punctuation uses herdr's names and drops the implied shift", () => {
  assert.equal(fromEvent(ev({ key: "-", code: "Minus" })), "minus");
  // shift+7 produces "&": reporting shift as well would be wrong.
  assert.equal(fromEvent(ev({ key: "&", code: "Digit7", shiftKey: true })), "ampersand");
  assert.equal(fromEvent(ev({ key: "`", code: "Backquote" })), "backtick");
});

test("unnamed punctuation is passed through literally", () => {
  // The user's own config has `split_vertical = "prefix+|"`.
  assert.equal(fromEvent(ev({ key: "|", code: "Backslash", shiftKey: true })), "|");
});

test("special keys and function keys are normalized", () => {
  assert.equal(fromEvent(ev({ key: "Escape", code: "Escape" })), "esc");
  assert.equal(fromEvent(ev({ key: "Enter", code: "Enter" })), "enter");
  assert.equal(fromEvent(ev({ key: "ArrowLeft", code: "ArrowLeft" })), "left");
  assert.equal(fromEvent(ev({ key: "F12", code: "F12" })), "f12");
  assert.equal(fromEvent(ev({ key: " ", code: "Space" })), "space");
});

test("digits come from the physical code", () => {
  assert.equal(fromEvent(ev({ key: "1", code: "Digit1", ctrlKey: true })), "ctrl+1");
});

test("withPrefix is idempotent", () => {
  assert.equal(withPrefix("shift+r"), "prefix+shift+r");
  assert.equal(withPrefix("prefix+shift+r"), "prefix+shift+r");
});

// --- parsing ---------------------------------------------------------------

test("modifier order does not change identity", () => {
  // herdr's own docs write both `ctrl+shift+alt+left` and `alt+shift+left`.
  assert.equal(canonical(parse("alt+shift+left")!), canonical(parse("shift+alt+left")!));
  assert.equal(canonical(parse("alt+shift+left")!), "shift+alt+left");
});

test("modifier aliases collapse", () => {
  for (const alias of ["control+a", "ctrl+a"]) assert.equal(canonical(parse(alias)!), "ctrl+a");
  for (const alias of ["cmd+a", "command+a", "super+a", "meta+a", "win+a"])
    assert.equal(canonical(parse(alias)!), "cmd+a", alias);
  for (const alias of ["alt+a", "option+a", "opt+a"]) assert.equal(canonical(parse(alias)!), "alt+a", alias);
});

test("the prefix token is tracked separately from modifiers", () => {
  const p = parse("prefix+shift+r")!;
  assert.equal(p.prefix, true);
  assert.deepEqual(p.mods, ["shift"]);
  assert.equal(p.key, "r");
});

test("a trailing plus is the plus key", () => {
  assert.equal(canonical(parse("prefix++")!), "prefix+plus");
  assert.equal(canonical(parse("prefix+plus")!), "prefix+plus");
});

test("a modifier-only value is the 1..9 range", () => {
  const p = parse("ctrl")!;
  assert.equal(p.range, true);
  assert.equal(p.key, "1..9");
});

test("empty and malformed values parse to null", () => {
  assert.equal(parse(""), null);
  assert.equal(parse("   "), null);
  assert.equal(parse("a+b"), null, "two non-modifier tokens is not a chord");
});

test("a range occupies nine chords", () => {
  assert.deepEqual(expand(parse("prefix+1..9")!), [
    "prefix+1", "prefix+2", "prefix+3", "prefix+4", "prefix+5",
    "prefix+6", "prefix+7", "prefix+8", "prefix+9",
  ]);
  assert.deepEqual(expand(parse("ctrl")!).slice(0, 2), ["ctrl+1", "ctrl+2"]);
});

// --- validation ------------------------------------------------------------

test("the prefix key may not carry prefix+", () => {
  assert.deepEqual(validate("ctrl+a", "prefix"), []);
  assert.deepEqual(validate("f12", "prefix"), []);
  assert.deepEqual(validate("esc", "prefix"), []);
  assert.match(validate("prefix+a", "prefix")[0], /prefix\+ は付けられません/);
});

test("navigate mode rejects the keys herdr reserves", () => {
  for (const ok of ["j", "k", "up", "down", "h", "l"]) {
    assert.deepEqual(validate(ok, "navigate"), [], ok);
  }
  for (const bad of ["prefix+j", "esc", "enter", "tab", "left", "right", "3"]) {
    assert.ok(validate(bad, "navigate").length > 0, `${bad} should be rejected`);
  }
});

test("indexed bindings take modifiers only", () => {
  assert.deepEqual(validate("ctrl", "indexed", true), []);
  assert.deepEqual(validate("ctrl+shift", "indexed", true), []);
  assert.ok(validate("ctrl+t", "indexed", true).length > 0);
  assert.ok(validate("prefix+ctrl", "indexed", true).length > 0);
});

test("an unmodified direct binding is rejected for actions", () => {
  assert.ok(validate("n", "action").length > 0);
  assert.deepEqual(validate("prefix+n", "action"), []);
  assert.deepEqual(validate("ctrl+alt+n", "action"), []);
  assert.deepEqual(validate("f12", "action"), [], "function keys need no modifier");
});

test("the range form is only allowed where the schema says so", () => {
  assert.deepEqual(validate("prefix+1..9", "action", true), []);
  assert.match(validate("prefix+1..9", "action", false)[0], /レンジ表記を受け付けません/);
});

test("an empty binding is always acceptable", () => {
  for (const kind of ["prefix", "action", "navigate", "indexed", "command"] as Kind[]) {
    assert.deepEqual(validate("", kind), [], kind);
  }
});

// --- terminal reliability --------------------------------------------------

test("prefix-mode bindings are reliable whatever the chord", () => {
  assert.equal(risk("prefix+alt+g", "action").level, "safe");
  assert.equal(risk("prefix+&", "action").level, "safe");
});

test("direct bindings are graded by what terminals actually deliver", () => {
  assert.equal(risk("ctrl+b", "action").level, "safe");
  assert.equal(risk("f12", "action").level, "safe");
  assert.equal(risk("ctrl+shift+r", "action").level, "caution");
  assert.equal(risk("alt+shift+left", "action").level, "risky");
  assert.equal(risk("cmd+k", "action").level, "risky");
  assert.equal(risk("ctrl+minus", "action").level, "risky");
  assert.equal(risk("n", "action").level, "risky");
});

test("navigate keys are judged in their own modal scope", () => {
  assert.equal(risk("j", "navigate").level, "safe");
  assert.equal(risk("j", "action").level, "risky", "the same key is unsafe as a global binding");
});

// --- conflicts -------------------------------------------------------------

const entry = (path: string, value: string, kind: Kind = "action"): Entry => ({ path, value, kind });

test("two bindings on the same chord conflict", () => {
  const c = conflicts([entry("keys.new_tab", "prefix+c"), entry("keys.close_pane", "prefix+c")]);
  assert.equal(c.length, 1);
  assert.equal(c[0].chord, "prefix+c");
  assert.deepEqual(c[0].paths, ["keys.new_tab", "keys.close_pane"]);
});

test("conflicts ignore modifier order", () => {
  const c = conflicts([entry("a", "alt+shift+left"), entry("b", "shift+alt+left")]);
  assert.equal(c.length, 1, "the same chord written two ways must still collide");
});

test("a range collides with a single binding inside it", () => {
  const c = conflicts([
    entry("keys.switch_tab", "prefix+1..9"),
    entry("keys.goto", "prefix+3"),
  ]);
  assert.equal(c.length, 1);
  assert.equal(c[0].chord, "prefix+3");
});

test("navigate keys do not collide with global bindings", () => {
  // herdr: navigate-mode shortcuts are independent from focus_pane_*.
  const c = conflicts([
    entry("keys.navigate_pane_down", "j", "navigate"),
    entry("keys.focus_pane_down", "j"),
  ]);
  assert.deepEqual(c, [], "different scopes");
  assert.equal(scopeOf("navigate"), "navigate");
  assert.equal(scopeOf("action"), "global");
});

test("navigate keys still collide with each other", () => {
  const c = conflicts([
    entry("keys.navigate_pane_down", "j", "navigate"),
    entry("keys.navigate_workspace_down", "j", "navigate"),
  ]);
  assert.equal(c.length, 1);
  assert.equal(c[0].scope, "navigate");
});

test("unset bindings never conflict", () => {
  assert.deepEqual(conflicts([entry("a", ""), entry("b", ""), entry("c", "   ")]), []);
});

test("the prefix key occupies its chord globally", () => {
  const c = conflicts([
    entry("keys.prefix", "ctrl+a", "prefix"),
    entry("keys.remote_image_paste", "ctrl+a"),
  ]);
  assert.equal(c.length, 1);
  assert.equal(c[0].chord, "ctrl+a");
});

// --- the bindings herdr actually ships ------------------------------------

test(`every default binding in herdr ${fixture.herdrVersion} validates`, () => {
  const bad: string[] = [];
  for (const b of fixture.bindings) {
    const errs = validate(b.default, b.kind as Kind, b.acceptsRange);
    if (errs.length) bad.push(`${b.path} = "${b.default}" (${b.kind}): ${errs.join("; ")}`);
  }
  assert.deepEqual(bad, [], "the rules must not reject herdr's own defaults");
});

test("the bindings herdr ships do not conflict with each other", () => {
  const c = conflicts(
    fixture.bindings.map((b) => ({ path: b.path, value: b.default, kind: b.kind as Kind }))
  );
  assert.deepEqual(c, [], "shipped defaults should be conflict-free");
});

test("the fixture really covers every binding family", () => {
  const kinds = new Set(fixture.bindings.map((b) => b.kind));
  assert.deepEqual([...kinds].sort(), ["action", "command", "indexed", "navigate", "prefix"]);
  assert.equal(fixture.bindings.length, 58);
});
