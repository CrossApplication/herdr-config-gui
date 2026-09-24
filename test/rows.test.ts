import assert from "node:assert/strict";
import test from "node:test";
import { CUSTOM_OPTION, applyRowsAction, applyTokenChange } from "../src/rowsui";
import {
  FG_RE,
  moveRow,
  moveToken,
  parseRows,
  rowsToToml,
  validateRows,
  type Rows,
} from "../src/rows";

const AGENT = [
  "state_icon",
  "state_text",
  "machine",
  "workspace",
  "tab",
  "pane",
  "agent",
  "terminal_title",
  "terminal_title_stripped",
];

// --- parsing ---------------------------------------------------------------

test("the defaults herdr ships parse to their structure", () => {
  assert.deepEqual(parseRows('[["state_icon", "machine", "workspace", "tab"], ["agent"]]'), [
    [{ token: "state_icon" }, { token: "machine" }, { token: "workspace" }, { token: "tab" }],
    [{ token: "agent" }],
  ]);
  assert.deepEqual(parseRows('[["state_icon", "workspace"], ["branch", "git_status"]]'), [
    [{ token: "state_icon" }, { token: "workspace" }],
    [{ token: "branch" }, { token: "git_status" }],
  ]);
});

test("an inline style is read into the token", () => {
  assert.deepEqual(
    parseRows('[[{ token = "workspace", fg = "#89b4fa", bold = true, dim = false }]]'),
    [[{ token: "workspace", fg: "#89b4fa", bold: true, dim: false }]]
  );
});

test("empty rows and empty lists are legal", () => {
  assert.deepEqual(parseRows("[]"), []);
  assert.deepEqual(parseRows("[[]]"), [[]]);
  assert.deepEqual(parseRows("[ [ ] , [ ] ]"), [[], []]);
});

test("whitespace, newlines and trailing commas are tolerated", () => {
  const written = `[
    ["state_icon", "machine"],
    ["agent"],
  ]`;
  assert.deepEqual(parseRows(written), [
    [{ token: "state_icon" }, { token: "machine" }],
    [{ token: "agent" }],
  ]);
});

test("shapes the editor cannot represent parse to null, not to something wrong", () => {
  // Falling back to the raw TOML field is safer than rewriting a value we
  // only half understood.
  for (const bad of [
    '["state_icon"]', // one-dimensional; herdr rejects it too
    '[["state_icon"]] trailing',
    '[[{ token = "workspace", italic = true }]]', // not a field herdr accepts
    '[[{ fg = "#fff" }]]', // no token
    "[[123]]",
    '[["unterminated]]',
    "[[",
    "not an array",
  ]) {
    assert.equal(parseRows(bad), null, bad);
  }
});

// --- serializing -----------------------------------------------------------

test("a plain token is a string and a styled one is an inline table", () => {
  assert.equal(
    rowsToToml([[{ token: "state_icon" }, { token: "agent", dim: true }]]),
    '[["state_icon", { token = "agent", dim = true }]]'
  );
});

test("style fields keep a fixed order", () => {
  assert.equal(
    rowsToToml([[{ token: "workspace", dim: false, bold: true, fg: "#89b4fa" }]]),
    '[[{ token = "workspace", fg = "#89b4fa", bold = true, dim = false }]]'
  );
});

test("parsing and serializing round-trip", () => {
  for (const source of [
    "[]",
    '[["state_icon", "machine", "workspace", "tab"], ["agent"]]',
    '[["state_icon", "workspace"], ["branch", "git_status"]]',
    '[[{ token = "workspace", fg = "#89b4fa", bold = true, dim = false }]]',
    '[["state_icon", { token = "agent", dim = true }], ["$jj_status"]]',
  ]) {
    const parsed = parseRows(source);
    assert.notEqual(parsed, null, source);
    assert.equal(rowsToToml(parsed!), source, source);
  }
});

// --- validation ------------------------------------------------------------

test("tokens outside the family are rejected", () => {
  // herdr: `unknown sidebar token`; the two families are not interchangeable.
  const errors = validateRows([[{ token: "branch" }]], AGENT);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /使えません/);
});

test("a custom token needs a name after the sigil", () => {
  assert.deepEqual(validateRows([[{ token: "$jj_status" }]], AGENT), []);
  assert.equal(validateRows([[{ token: "$" }]], AGENT).length, 1);
});

test("fg takes only hex, unlike theme colors", () => {
  assert.deepEqual(validateRows([[{ token: "agent", fg: "#89b4fa" }]], AGENT), []);
  assert.deepEqual(validateRows([[{ token: "agent", fg: "#abc" }]], AGENT), []);
  for (const bad of ["cyan", "rgb(1,2,3)", "reset", "", "#12345"]) {
    assert.equal(validateRows([[{ token: "agent", fg: bad }]], AGENT).length, 1, bad);
  }
});

test("the position of a problem is reported", () => {
  const errors = validateRows([[{ token: "agent" }], [{ token: "agent" }, { token: "nope" }]], AGENT);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /^2 行目 2 番目/);
});

test("the shipped defaults raise nothing", () => {
  assert.deepEqual(validateRows(parseRows('[["state_icon", "machine", "workspace", "tab"], ["agent"]]')!, AGENT), []);
});

// --- structural edits ------------------------------------------------------

const sample = (): Rows => [
  [{ token: "state_icon" }, { token: "machine" }],
  [{ token: "agent" }],
];

test("rows move and stop at the ends", () => {
  assert.deepEqual(moveRow(sample(), 0, 1), [[{ token: "agent" }], [{ token: "state_icon" }, { token: "machine" }]]);
  assert.deepEqual(moveRow(sample(), 0, -1), sample(), "already first");
  assert.deepEqual(moveRow(sample(), 1, 1), sample(), "already last");
});

test("a token moves within its row", () => {
  assert.deepEqual(moveToken(sample(), 0, 1, -1), [
    [{ token: "machine" }, { token: "state_icon" }],
    [{ token: "agent" }],
  ]);
});

test("a token at the edge of a row hops to the neighbour", () => {
  // Moving right from the end of row 1 puts it at the start of row 2.
  assert.deepEqual(moveToken(sample(), 0, 1, 1), [
    [{ token: "state_icon" }],
    [{ token: "machine" }, { token: "agent" }],
  ]);
  // And moving left from the start of row 2 appends it to row 1.
  assert.deepEqual(moveToken(sample(), 1, 0, -1), [
    [{ token: "state_icon" }, { token: "machine" }, { token: "agent" }],
    [],
  ]);
});

test("a token at the outer edge stays put", () => {
  assert.deepEqual(moveToken(sample(), 0, 0, -1), sample());
  assert.deepEqual(moveToken(sample(), 1, 0, 1), sample());
});

test("edits do not mutate the input", () => {
  const original = sample();
  moveRow(original, 0, 1);
  moveToken(original, 0, 0, 1);
  assert.deepEqual(original, sample());
});

// --- editor actions --------------------------------------------------------

test("a row is added empty and removed by index", () => {
  assert.deepEqual(applyRowsAction(sample(), "add-row", 0, 0, 0, "agent"), [...sample(), []]);
  assert.deepEqual(applyRowsAction(sample(), "del-row", 0, 0, 0, "agent"), [[{ token: "agent" }]]);
});

test("a token is appended to the row it was requested from", () => {
  assert.deepEqual(applyRowsAction(sample(), "add-token", 1, 0, 0, "tab"), [
    [{ token: "state_icon" }, { token: "machine" }],
    [{ token: "agent" }, { token: "tab" }],
  ]);
});

test("an unknown action changes nothing", () => {
  assert.equal(applyRowsAction(sample(), "nonsense", 0, 0, 0, "agent"), null);
});

test("choosing a built-in replaces the token name", () => {
  assert.deepEqual(applyTokenChange(sample(), 0, 0, "token", "pane"), [
    [{ token: "pane" }, { token: "machine" }],
    [{ token: "agent" }],
  ]);
});

test("switching to custom seeds the sigil, and back again replaces it", () => {
  const custom = applyTokenChange(sample(), 0, 0, "token", CUSTOM_OPTION);
  assert.equal(custom[0][0].token, "$", "a name has to start somewhere");
  const named = applyTokenChange(custom, 0, 0, "custom", "$jj_status");
  assert.equal(named[0][0].token, "$jj_status");
  // Re-opening the select keeps the name rather than losing it.
  assert.equal(applyTokenChange(named, 0, 0, "token", CUSTOM_OPTION)[0][0].token, "$jj_status");
  assert.equal(applyTokenChange(named, 0, 0, "token", "agent")[0][0].token, "agent");
});

test("style flags are removed rather than written as false", () => {
  // herdr treats an omitted field as "keep the contextual default", which is
  // not the same as explicitly false.
  const bold = applyTokenChange(sample(), 0, 0, "bold", true);
  assert.equal(bold[0][0].bold, true);
  assert.equal(rowsToToml(bold), '[[{ token = "state_icon", bold = true }, "machine"], ["agent"]]');
  const off = applyTokenChange(bold, 0, 0, "bold", false);
  assert.equal("bold" in off[0][0], false);
  assert.equal(rowsToToml(off), rowsToToml(sample()));
});

test("turning fg on seeds a colour and turning it off drops the field", () => {
  const on = applyTokenChange(sample(), 0, 0, "fg-on", true);
  assert.match(on[0][0].fg!, FG_RE);
  const set = applyTokenChange(on, 0, 0, "fg", "#89b4fa");
  assert.equal(set[0][0].fg, "#89b4fa");
  assert.equal("fg" in applyTokenChange(set, 0, 0, "fg-on", false)[0][0], false);
});

test("a change touches only the token it names", () => {
  const next = applyTokenChange(sample(), 0, 1, "dim", true);
  assert.deepEqual(next[0][0], { token: "state_icon" });
  assert.deepEqual(next[1], [{ token: "agent" }]);
});
