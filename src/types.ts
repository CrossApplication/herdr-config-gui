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
export type Preview = { changes: Change[]; after: string; error: string | null };
export type SaveResult = {
  path: string;
  backup: string | null;
  changes: Change[];
  check_ok: boolean;
  check_output: string;
  reloaded: boolean | null;
  reload_output: string;
};

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
