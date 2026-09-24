/**
 * Sidebar rows: `[["state_icon", "machine"], [{ token = "agent", dim = true }]]`
 *
 * A row is a list of tokens; a token is either a built-in name, a custom value
 * reported through metadata (anything starting with `$`), or an inline table
 * carrying a style. herdr accepts only `token`, `fg`, `bold` and `dim` there,
 * and `fg` must be `#rgb` or `#rrggbb` -- named colors and `rgb()` are
 * rejected, unlike `[theme.custom]`.
 *
 * Parsing is deliberately narrow: this handles the shape the editor can
 * represent and returns null for anything else, so a hand-written value the
 * parser does not understand falls back to the raw TOML field instead of
 * being silently rewritten.
 */

export type RowToken = {
  /** A built-in token name, or `$name` for a metadata value. */
  token: string;
  /** `#rgb` or `#rrggbb`; herdr rejects every other notation here. */
  fg?: string;
  bold?: boolean;
  dim?: boolean;
};
export type Rows = RowToken[][];

export const FG_RE = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;
export const isCustomToken = (t: string) => t.startsWith("$") && t.length > 1;

// --- parsing ---------------------------------------------------------------

class Cursor {
  constructor(
    readonly text: string,
    public i = 0
  ) {}
  ws() {
    while (this.i < this.text.length && /\s/.test(this.text[this.i])) this.i++;
  }
  peek(): string {
    this.ws();
    return this.text[this.i] ?? "";
  }
  take(ch: string): boolean {
    if (this.peek() !== ch) return false;
    this.i++;
    return true;
  }
}

function parseString(c: Cursor): string | null {
  if (!c.take('"')) return null;
  let out = "";
  while (c.i < c.text.length) {
    const ch = c.text[c.i++];
    if (ch === '"') return out;
    if (ch === "\\") {
      const next = c.text[c.i++];
      out += next === "n" ? "\n" : next === "t" ? "\t" : next;
      continue;
    }
    out += ch;
  }
  return null; // unterminated
}

function parseInlineTable(c: Cursor): RowToken | null {
  if (!c.take("{")) return null;
  const out: Record<string, string | boolean> = {};
  if (c.take("}")) return null; // a token table must at least name a token
  for (;;) {
    c.ws();
    const key = /^[A-Za-z_][A-Za-z0-9_-]*/.exec(c.text.slice(c.i))?.[0];
    if (!key) return null;
    c.i += key.length;
    if (!c.take("=")) return null;
    c.ws();
    if (c.peek() === '"') {
      const v = parseString(c);
      if (v === null) return null;
      out[key] = v;
    } else if (c.text.startsWith("true", c.i)) {
      c.i += 4;
      out[key] = true;
    } else if (c.text.startsWith("false", c.i)) {
      c.i += 5;
      out[key] = false;
    } else {
      return null; // numbers, dates, nested tables: not something we edit
    }
    if (c.take(",")) continue;
    if (c.take("}")) break;
    return null;
  }

  // Only the four fields herdr accepts, and only with the right types.
  const token = out.token;
  if (typeof token !== "string") return null;
  const result: RowToken = { token };
  for (const [k, v] of Object.entries(out)) {
    if (k === "token") continue;
    if (k === "fg" && typeof v === "string") result.fg = v;
    else if ((k === "bold" || k === "dim") && typeof v === "boolean") result[k] = v;
    else return null;
  }
  return result;
}

/** Structured rows, or null when the text is not a shape the editor handles. */
export function parseRows(toml: string): Rows | null {
  const c = new Cursor(toml.trim());
  if (!c.take("[")) return null;
  const rows: Rows = [];
  if (c.take("]")) return c.peek() === "" ? rows : null;

  for (;;) {
    if (!c.take("[")) return null;
    const row: RowToken[] = [];
    if (!c.take("]")) {
      for (;;) {
        const ch = c.peek();
        if (ch === '"') {
          const s = parseString(c);
          if (s === null) return null;
          row.push({ token: s });
        } else if (ch === "{") {
          const t = parseInlineTable(c);
          if (t === null) return null;
          row.push(t);
        } else {
          return null;
        }
        if (c.take(",")) {
          if (c.take("]")) break; // trailing comma
          continue;
        }
        if (c.take("]")) break;
        return null;
      }
    }
    rows.push(row);
    if (c.take(",")) {
      if (c.take("]")) break; // trailing comma
      continue;
    }
    if (c.take("]")) break;
    return null;
  }
  return c.peek() === "" ? rows : null;
}

// --- serializing -----------------------------------------------------------

const quote = (s: string) => `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;

const hasStyle = (t: RowToken) =>
  t.fg !== undefined || t.bold !== undefined || t.dim !== undefined;

function tokenToToml(t: RowToken): string {
  if (!hasStyle(t)) return quote(t.token);
  const parts = [`token = ${quote(t.token)}`];
  if (t.fg !== undefined) parts.push(`fg = ${quote(t.fg)}`);
  if (t.bold !== undefined) parts.push(`bold = ${t.bold}`);
  if (t.dim !== undefined) parts.push(`dim = ${t.dim}`);
  return `{ ${parts.join(", ")} }`;
}

export function rowsToToml(rows: Rows): string {
  return `[${rows.map((row) => `[${row.map(tokenToToml).join(", ")}]`).join(", ")}]`;
}

// --- validation ------------------------------------------------------------

/** Problems herdr would reject the value for. */
export function validateRows(rows: Rows, allowed: readonly string[]): string[] {
  const errors: string[] = [];
  rows.forEach((row, r) => {
    row.forEach((t, i) => {
      const where = `${r + 1} 行目 ${i + 1} 番目`;
      if (t.token === "") errors.push(`${where}: トークンが空です`);
      else if (!allowed.includes(t.token) && !isCustomToken(t.token)) {
        errors.push(
          t.token.startsWith("$")
            ? `${where}: カスタムトークンには $ の後に名前が必要です`
            : `${where}: "${t.token}" はこの一覧では使えません（カスタム値は $ で始めます）`
        );
      }
      if (t.fg !== undefined && !FG_RE.test(t.fg)) {
        errors.push(`${where}: fg は #rgb か #rrggbb のみです（"${t.fg}"）`);
      }
    });
  });
  return errors;
}

// --- structural edits ------------------------------------------------------

const clone = (rows: Rows): Rows => rows.map((row) => row.map((t) => ({ ...t })));

export function moveRow(rows: Rows, index: number, delta: number): Rows {
  const to = index + delta;
  if (to < 0 || to >= rows.length) return rows;
  const next = clone(rows);
  const [row] = next.splice(index, 1);
  next.splice(to, 0, row);
  return next;
}

/** Move a token within its row; at an edge it hops to the neighbouring row. */
export function moveToken(rows: Rows, row: number, index: number, delta: number): Rows {
  const next = clone(rows);
  const to = index + delta;
  if (to >= 0 && to < next[row].length) {
    const [t] = next[row].splice(index, 1);
    next[row].splice(to, 0, t);
    return next;
  }
  const targetRow = row + delta;
  if (targetRow < 0 || targetRow >= next.length) return rows;
  const [t] = next[row].splice(index, 1);
  if (delta < 0) next[targetRow].push(t);
  else next[targetRow].unshift(t);
  return next;
}
