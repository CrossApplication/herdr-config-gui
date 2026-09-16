/**
 * The editing state model, kept free of Tauri/DOM so it can be tested.
 *
 * A setting is presented as a single field:
 *   field empty          -> inherit herdr's default (key removed from the file)
 *   field has a value    -> explicitly set
 *   explicit `""`        -> explicitly disabled (only meaningful for the few
 *                           settings whose docs say an empty string turns the
 *                           feature off; unreachable by clearing the field)
 */
import { toDisplay, toToml, type Item } from "./types";

export const EMPTY = '""';
export type State = "inherit" | "set" | "disabled";

export type Store = {
  /** Values currently on disk, keyed by dotted path, as TOML source text. */
  values: Record<string, string>;
  /** Touched settings only. `null` means "inherit the default". */
  edits: Map<string, string | null>;
};

export const newStore = (values: Record<string, string> = {}): Store => ({
  values,
  edits: new Map(),
});

export const saved = (st: Store, path: string): string | null => st.values[path] ?? null;

export const effective = (st: Store, path: string): string | null =>
  st.edits.has(path) ? st.edits.get(path)! : saved(st, path);

export const isDirty = (st: Store, path: string): boolean =>
  st.edits.has(path) && st.edits.get(path)! !== saved(st, path);

export const dirtyPaths = (st: Store): string[] =>
  [...st.edits.keys()].filter((p) => isDirty(st, p));

export function stateOf(st: Store, path: string): State {
  const v = effective(st, path);
  if (v === null) return "inherit";
  return v === EMPTY ? "disabled" : "set";
}

/**
 * Record an edit. Returning a setting to the value already on disk drops the
 * edit entirely, so round-tripping a field never leaves a phantom change.
 */
export function setEdit(st: Store, path: string, value: string | null): void {
  if (value === saved(st, path)) st.edits.delete(path);
  else st.edits.set(path, value);
}

/** Field text -> stored value. Throws when the text is not a valid value. */
export function fromField(text: string, ty: Item["ty"]): string | null {
  return text.trim() === "" ? null : toToml(text, ty);
}

/** Human-readable rendering of a stored value, for chips and diffs. */
export function label(v: string | null, item: Item): string {
  if (v === null) return `既定 (${toDisplay(item.default, item.ty) || '""'})`;
  if (v === EMPTY) return "無効 (空文字)";
  return toDisplay(v, item.ty);
}

/** What `save_edits` should receive. */
export const payload = (st: Store) =>
  dirtyPaths(st).map((path) => ({ path, value: st.edits.get(path)! }));
