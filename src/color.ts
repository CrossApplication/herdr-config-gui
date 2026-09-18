/**
 * Color values for theme tokens.
 *
 * herdr does not validate them at all -- `accent = "notacolor"` passes
 * `herdr config check` and is then silently ignored -- so this is the only
 * place a mistake can be caught. Because herdr's accepted set of *names* is
 * not documented, an unrecognised name is accepted rather than flagged; only
 * values that are clearly a broken hex or rgb() are called malformed.
 */

export type ColorForm = "empty" | "reset" | "hex" | "rgb" | "name" | "malformed";

/** Names used only to preview a swatch; herdr may accept more than these. */
const NAMED: Record<string, string> = {
  black: "#000000",
  red: "#cc0000",
  green: "#4e9a06",
  yellow: "#c4a000",
  blue: "#3465a4",
  magenta: "#75507b",
  cyan: "#06989a",
  white: "#d3d7cf",
  gray: "#808080",
  grey: "#808080",
  brightblack: "#555753",
  brightred: "#ef2929",
  brightgreen: "#8ae234",
  brightyellow: "#fce94f",
  brightblue: "#729fcf",
  brightmagenta: "#ad7fa8",
  brightcyan: "#34e2e2",
  brightwhite: "#eeeeec",
};

const HEX6 = /^#[0-9a-f]{6}$/i;
const HEX3 = /^#[0-9a-f]{3}$/i;
const RGB = /^rgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)$/i;
const NAME = /^[a-z][a-z0-9_-]*$/i;

export function colorForm(text: string): ColorForm {
  const t = text.trim();
  if (t === "") return "empty";
  if (t.toLowerCase() === "reset") return "reset";
  if (HEX6.test(t) || HEX3.test(t)) return "hex";
  if (t.startsWith("#")) return "malformed";
  const rgb = RGB.exec(t);
  if (rgb) {
    return rgb.slice(1).every((n) => Number(n) <= 255) ? "rgb" : "malformed";
  }
  if (/^rgb/i.test(t)) return "malformed";
  // An unknown name may still be valid: herdr's list is not documented.
  return NAME.test(t) ? "name" : "malformed";
}

const pad = (n: number) => n.toString(16).padStart(2, "0");

/** `#rrggbb` for the swatch, or null when the value cannot be previewed. */
export function colorToHex(text: string): string | null {
  const t = text.trim();
  if (HEX6.test(t)) return t.toLowerCase();
  if (HEX3.test(t)) {
    const [r, g, b] = t.slice(1).split("");
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  const rgb = RGB.exec(t);
  if (rgb) {
    const parts = rgb.slice(1).map(Number);
    if (parts.every((n) => n <= 255)) return `#${parts.map(pad).join("")}`;
  }
  return NAMED[t.toLowerCase().replace(/[\s_-]/g, "")] ?? null;
}

export const FORM_LABEL: Record<ColorForm, string> = {
  empty: "テーマ既定",
  reset: "端末の既定色に戻す",
  hex: "16進",
  rgb: "rgb()",
  name: "名前付き色",
  malformed: "色として解釈できません",
};
