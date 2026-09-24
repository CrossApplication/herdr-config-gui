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
import { sizeToToml, toDisplay, toToml, type Item } from "./types";

export const EMPTY = '""';
export type State = "inherit" | "set" | "disabled";

export type Store = {
  /** Values currently on disk, keyed by dotted path, as TOML source text. */
  values: Record<string, string>;
  /** Touched settings only. `null` means "inherit the default". */
  edits: Map<string, string | null>;
  /**
   * Array-of-tables entries marked for deletion, e.g. `keys.command[1]`.
   * Only entries that exist on disk need this; a locally added one is undone
   * by dropping its edits.
   */
  removedEntries: Set<string>;
};

export const newStore = (values: Record<string, string> = {}): Store => ({
  values,
  edits: new Map(),
  removedEntries: new Set(),
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

/**
 * Field text -> stored value for a specific setting.
 *
 * Popup dimensions carry two TOML types in one field, so the quoting depends
 * on the setting rather than on its declared type: `80%` is a string and a
 * cell count is a bare integer, and herdr rejects `width = "120"` outright.
 */
export function fromFieldFor(item: Pick<Item, "ty" | "size">, text: string): string | null {
  if (!item.size) return fromField(text, item.ty);
  return text.trim() === "" ? null : sizeToToml(text);
}

/** Human-readable rendering of a stored value, for chips and diffs. */
export function label(v: string | null, item: Item): string {
  if (v === null) return `既定 (${toDisplay(item.default, item.ty) || '""'})`;
  if (v === EMPTY) return "無効 (空文字)";
  return toDisplay(v, item.ty);
}

// --- array-of-tables entries -----------------------------------------------

const entryPath = (section: string, index: number) => `${section}[${index}]`;

const entryRe = (section: string) =>
  new RegExp(`^${section.replace(/[.[\]]/g, "\\$&")}\\[(\\d+)\\]\\.`);

/** Indices of the entries the form should show, lowest first. */
export function entryIndices(st: Store, section: string): number[] {
  const re = entryRe(section);
  const found = new Set<number>();
  for (const key of [...Object.keys(st.values), ...st.edits.keys()]) {
    const m = re.exec(key);
    if (m) found.add(Number(m[1]));
  }
  for (const index of [...found]) {
    if (st.removedEntries.has(entryPath(section, index))) found.delete(index);
  }
  return [...found].sort((a, b) => a - b);
}

const entryExistsOnDisk = (st: Store, section: string, index: number): boolean =>
  Object.keys(st.values).some((k) => k.startsWith(`${entryPath(section, index)}.`));

/**
 * Append an entry, seeded so it is a valid row rather than an empty one.
 * The index is always one past the highest in use, because herdr's file has
 * no way to express a gap.
 */
export function addEntry(st: Store, section: string, seed: Record<string, string>): number {
  const used = entryIndices(st, section);
  const removed = [...st.removedEntries]
    .map((p) => entryRe(section).exec(`${p}.`)?.[1])
    .filter((x): x is string => x !== undefined)
    .map(Number);
  const index = Math.max(-1, ...used, ...removed) + 1;
  for (const [key, value] of Object.entries(seed)) {
    setEdit(st, `${entryPath(section, index)}.${key}`, value);
  }
  return index;
}

export function removeEntry(st: Store, section: string, index: number): void {
  const prefix = `${entryPath(section, index)}.`;
  for (const key of [...st.edits.keys()]) {
    if (key.startsWith(prefix)) st.edits.delete(key);
  }
  // An entry that was never saved just disappears; one on disk needs an op.
  if (entryExistsOnDisk(st, section, index)) {
    st.removedEntries.add(entryPath(section, index));
  }
}

export const hasPendingWork = (st: Store): boolean =>
  dirtyPaths(st).length > 0 || st.removedEntries.size > 0;

// --- what the backend receives ---------------------------------------------

export type Payload = {
  path: string;
  value: string | null;
  op: "set" | "remove_entry";
};

export function payload(st: Store): Payload[] {
  const removedPrefixes = [...st.removedEntries].map((p) => `${p}.`);
  const sets = dirtyPaths(st)
    // No point writing values into an entry that is about to be deleted.
    .filter((path) => !removedPrefixes.some((prefix) => path.startsWith(prefix)))
    .map((path) => ({ path, value: st.edits.get(path)!, op: "set" as const }));
  const removals = [...st.removedEntries].map((path) => ({
    path,
    value: null,
    op: "remove_entry" as const,
  }));
  return [...sets, ...removals];
}
