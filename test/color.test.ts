import assert from "node:assert/strict";
import test from "node:test";
import { colorForm, colorToHex } from "../src/color";
import { sizeToDisplay, sizeToToml } from "../src/types";

test("the documented forms are recognised", () => {
  // From herdr's own comment: hex (#rrggbb), named colors, rgb(r,g,b),
  // or panel_bg = "reset".
  assert.equal(colorForm("#f5c2e7"), "hex");
  assert.equal(colorForm("#fff"), "hex");
  assert.equal(colorForm("rgb(137, 180, 250)"), "rgb");
  assert.equal(colorForm("cyan"), "name");
  assert.equal(colorForm("reset"), "reset");
  assert.equal(colorForm(""), "empty");
});

test("a broken hex or rgb is called out", () => {
  assert.equal(colorForm("#12345"), "malformed");
  assert.equal(colorForm("#zzzzzz"), "malformed");
  assert.equal(colorForm("rgb(1,2)"), "malformed");
  assert.equal(colorForm("rgb(300,0,0)"), "malformed", "components cap at 255");
});

test("an unknown name is accepted, because herdr's list is not documented", () => {
  // Flagging these would warn about values that may well work.
  assert.equal(colorForm("rosewater"), "name");
  assert.equal(colorForm("subtext1"), "name");
});

test("values that are not colors at all are malformed", () => {
  assert.equal(colorForm("12345"), "malformed");
  assert.equal(colorForm("#"), "malformed");
  assert.equal(colorForm("a b"), "malformed");
});

test("the swatch preview expands and converts", () => {
  assert.equal(colorToHex("#F5C2E7"), "#f5c2e7");
  assert.equal(colorToHex("#abc"), "#aabbcc");
  assert.equal(colorToHex("rgb(137,180,250)"), "#89b4fa");
  assert.equal(colorToHex("rgb(0,0,0)"), "#000000");
  assert.equal(colorToHex("cyan"), "#06989a");
});

test("what cannot be previewed returns null rather than a wrong swatch", () => {
  assert.equal(colorToHex("reset"), null);
  assert.equal(colorToHex(""), null);
  assert.equal(colorToHex("rosewater"), null, "an unknown name has no preview");
  assert.equal(colorToHex("#12345"), null);
});

// --- popup dimensions ------------------------------------------------------

test("a percentage is quoted and a cell count is not", () => {
  // herdr: `string sizes must be percentages like 80%; use a number for cells`
  assert.equal(sizeToToml("80%"), '"80%"');
  assert.equal(sizeToToml("120"), "120");
  assert.equal(sizeToToml(" 40 "), "40");
});

test("percentages outside 1-100 are refused", () => {
  assert.throws(() => sizeToToml("0%"));
  assert.throws(() => sizeToToml("200%"));
  assert.equal(sizeToToml("1%"), '"1%"');
  assert.equal(sizeToToml("100%"), '"100%"');
});

test("anything that is neither is refused rather than guessed at", () => {
  for (const bad of ["big", "80 %", "80px", "", "-10"]) {
    assert.throws(() => sizeToToml(bad), Error, bad);
  }
});

test("both TOML types display as what was typed", () => {
  assert.equal(sizeToDisplay('"80%"'), "80%");
  assert.equal(sizeToDisplay("120"), "120");
});
