/**
 * The sidebar rows editor.
 *
 * Kept apart from main.ts because it is the only setting with a shape of its
 * own: a list of lines, each a list of tokens, each optionally styled. The
 * pure parts (parse, serialize, validate, move) live in rows.ts; this builds
 * the markup and turns interactions into new structures.
 */
import { isCustomToken, moveRow, moveToken, type RowToken, type Rows } from "./rows";

const esc = (s: string) =>
  s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

/** Sentinel for the "custom value" option; not a name herdr accepts. */
export const CUSTOM_OPTION = "::custom::";

const HEX = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;
const DEFAULT_FG = "#cdd6f4";

function tokenControls(t: RowToken, r: number, i: number, allowed: string[]): string {
  const custom = !allowed.includes(t.token);
  const options = [
    ...allowed.map(
      (name) =>
        `<option value="${esc(name)}" ${name === t.token ? "selected" : ""}>${esc(name)}</option>`
    ),
    `<option value="${CUSTOM_OPTION}" ${custom ? "selected" : ""}>$ カスタム値…</option>`,
  ].join("");

  const fgOn = t.fg !== undefined;
  const swatch = fgOn && HEX.test(t.fg!) ? t.fg! : DEFAULT_FG;
  return `<div class="tok" data-r="${r}" data-i="${i}">
      <select class="tok-name" data-rows-set="token">${options}</select>
      ${
        custom
          ? `<input type="text" class="field tok-custom" data-rows-set="custom"
               value="${esc(t.token)}" placeholder="$jj_status" spellcheck="false" />`
          : ""
      }
      <label class="tok-style" title="前景色。herdr はここでは #rgb / #rrggbb しか受け付けません">
        <input type="checkbox" data-rows-set="fg-on" ${fgOn ? "checked" : ""} />
        <input type="color" class="swatch tok-fg" data-rows-set="fg"
          value="${esc(swatch)}" ${fgOn ? "" : "disabled"} />
      </label>
      <label class="tok-style" title="太字"><input type="checkbox" data-rows-set="bold" ${
        t.bold ? "checked" : ""
      } /> B</label>
      <label class="tok-style" title="淡色"><input type="checkbox" data-rows-set="dim" ${
        t.dim ? "checked" : ""
      } /> D</label>
      <span class="cap-spacer"></span>
      <button class="ghost tiny" data-rows-act="move-token" data-d="-1" title="前へ">&larr;</button>
      <button class="ghost tiny" data-rows-act="move-token" data-d="1" title="次へ">&rarr;</button>
      <button class="ghost tiny danger" data-rows-act="del-token" title="このトークンを削除">&times;</button>
    </div>`;
}

/** What the row will look like in herdr's sidebar, left to right. */
const rowPreview = (row: RowToken[]): string =>
  row.length ? row.map((t) => t.token).join("  ") : "(空の行)";

export function renderRowsEditor(rows: Rows, allowed: string[]): string {
  const cards = rows.map((row, r) => {
    const tokens = row.map((t, i) => tokenControls(t, r, i, allowed)).join("");
    return `<div class="rowcard" data-r="${r}">
        <div class="rowcard-head">
          <span class="rowcard-no">${r + 1} 行目</span>
          <code class="rowcard-preview">${esc(rowPreview(row))}</code>
          <span class="cap-spacer"></span>
          <button class="ghost tiny" data-rows-act="move-row" data-d="-1" title="上へ">&uarr;</button>
          <button class="ghost tiny" data-rows-act="move-row" data-d="1" title="下へ">&darr;</button>
          <button class="ghost tiny danger" data-rows-act="del-row" title="この行を削除">&times;</button>
        </div>
        ${tokens}
        <button class="ghost add tiny" data-rows-act="add-token">＋ トークンを追加</button>
      </div>`;
  });

  const empty = rows.length ? "" : `<div class="sec-doc">行がありません。</div>`;
  return `<div class="rows-editor">
      ${empty}${cards.join("")}
      <button class="ghost add" data-rows-act="add-row">＋ 行を追加</button>
    </div>`;
}

/** Apply a button action; null when the action is not one we handle. */
export function applyRowsAction(
  rows: Rows,
  act: string,
  r: number,
  i: number,
  delta: number,
  newToken: string
): Rows | null {
  switch (act) {
    case "add-row":
      return [...rows, []];
    case "del-row":
      return rows.filter((_, x) => x !== r);
    case "move-row":
      return moveRow(rows, r, delta);
    case "add-token":
      return rows.map((row, x) => (x === r ? [...row, { token: newToken }] : row));
    case "del-token":
      return rows.map((row, x) => (x === r ? row.filter((_, y) => y !== i) : row));
    case "move-token":
      return moveToken(rows, r, i, delta);
    default:
      return null;
  }
}

/** Apply a control change to one token. */
export function applyTokenChange(
  rows: Rows,
  r: number,
  i: number,
  field: string,
  value: string | boolean
): Rows {
  return rows.map((row, x) =>
    x !== r
      ? row
      : row.map((t, y) => {
          if (y !== i) return t;
          const next: RowToken = { ...t };
          switch (field) {
            case "token":
              // Switching to custom keeps an existing $name, or starts one, so
              // the text field has something to show.
              next.token =
                value === CUSTOM_OPTION
                  ? isCustomToken(t.token)
                    ? t.token
                    : "$"
                  : String(value);
              break;
            case "custom":
              next.token = String(value);
              break;
            case "fg-on":
              if (value) next.fg = next.fg ?? DEFAULT_FG;
              else delete next.fg;
              break;
            case "fg":
              next.fg = String(value);
              break;
            case "bold":
              if (value) next.bold = true;
              else delete next.bold;
              break;
            case "dim":
              if (value) next.dim = true;
              else delete next.dim;
              break;
          }
          return next;
        })
  );
}
