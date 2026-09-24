export type Item = {
  line: number;
  path: string;
  section: string;
  key: string;
  ty: "bool" | "integer" | "float" | "string" | "array" | "table" | "datetime";
  default: string;
  doc: string[];
  trailing: string;
  enum_candidates: string[];
  optional: boolean;
  empty_disables: boolean;
  is_key_binding: boolean;
  binding_kind: "prefix" | "action" | "navigate" | "indexed" | "command" | null;
  accepts_range: boolean;
  /** The value is a popup dimension: `"80%"` as a string, cells as an integer. */
  size: boolean;
  /** The value is sidebar rows; names the token family (`agent` / `space`). */
  token_set: string | null;
  /** The value is a color: the form offers a picker and validates it. */
  color: boolean;
  /** Contributed by the hand-written overlay, not by `herdr --default-config`. */
  from_overlay: boolean;
};
export type Hint = { line: number; name: string; description: string };
export type Section = {
  name: string;
  line: number;
  commented: boolean;
  array_of_tables: boolean;
  doc: string[];
  items: Item[];
  hints: Hint[];
};
export type ConfigState = {
  path: string;
  exists: boolean;
  raw: string;
  set_paths: string[];
  values: Record<string, string>;
  parse_error: string | null;
};
export type Bootstrap = {
  /** Token family -> the tokens a sidebar row of that family may contain. */
  token_sets: Record<string, string[]>;
  /**
   * Canonical agent ids per table. herdr spells two of them differently in
   * each: `opencode`/`copilot` under rows_by_agent, `open_code`/
   * `github_copilot` under sound.
   */
  agent_ids: Record<string, string[]>;
  herdr_found: boolean;
  herdr_path: string | null;
  herdr_version: string | null;
  schema: { sections: Section[]; item_count: number; hint_count: number } | null;
  schema_error: string | null;
  config: ConfigState;
  unknown_paths: string[];
};
export type Change = {
  path: string;
  from: string | null;
  to: string | null;
  action: "add" | "update" | "remove" | "noop";
};
/** One thing `herdr config check` said about a candidate config. */
export type Diagnostic = {
  severity: "error" | "warning";
  kind: "unknown_section" | "unknown_key" | "type" | "variant" | "syntax" | "other";
  message: string;
  path: string | null;
  line: number | null;
  expected: string | null;
  allowed: string[];
};
export type CheckReport = {
  ok: boolean;
  /** herdr would fall back to defaults for the whole file. */
  discards_config: boolean;
  diagnostics: Diagnostic[];
  raw: string;
  /** Set when herdr could not be run at all. */
  unavailable: string | null;
};
export type Preview = {
  changes: Change[];
  after: string;
  error: string | null;
  check: CheckReport | null;
};
export type SaveResult = {
  path: string;
  /** False when the pre-flight check refused the content. */
  written: boolean;
  backup: string | null;
  changes: Change[];
  check: CheckReport;
  reloaded: boolean | null;
  reload_output: string;
};

export const isFatal = (c: CheckReport | null): boolean =>
  !!c && (c.discards_config || c.diagnostics.some((d) => d.severity === "error"));

/** TOML source text -> value shown in the widget. */
export function toDisplay(text: string, ty: Item["ty"]): string {
  if (ty === "string") {
    try {
      return JSON.parse(text) as string;
    } catch {
      return text.replace(/^"|"$/g, "");
    }
  }
  return text;
}

/** Widget value -> TOML source text. Throws when the input is not valid. */
export function toToml(display: string, ty: Item["ty"]): string {
  switch (ty) {
    case "string":
      return JSON.stringify(display);
    case "bool":
      return display === "true" ? "true" : "false";
    case "integer": {
      const t = display.trim();
      if (!/^-?\d+$/.test(t)) throw new Error("整数を入力してください");
      return t;
    }
    case "float": {
      const t = display.trim();
      if (!/^-?\d+(\.\d+)?$/.test(t)) throw new Error("数値を入力してください");
      return t;
    }
    default:
      return display.trim();
  }
}


/**
 * Popup dimensions are two TOML types in one field: a percentage is a string,
 * a cell count is a bare integer. herdr rejects `width = "120"` outright, so
 * the quoting has to follow what was typed.
 */
export function sizeToToml(display: string): string {
  const t = display.trim();
  if (/^\d+%$/.test(t)) {
    const n = Number(t.slice(0, -1));
    if (n < 1 || n > 100) throw new Error("パーセントは 1% から 100% の範囲です");
    return JSON.stringify(t);
  }
  if (/^\d+$/.test(t)) return t; // cells, deliberately unquoted
  throw new Error('"80%" のようなパーセント、またはセル数の整数を入力してください');
}

/** The stored TOML for a dimension, as text for the field. */
export function sizeToDisplay(text: string): string {
  const t = text.trim();
  return /^".*"$/.test(t) ? t.slice(1, -1) : t;
}
