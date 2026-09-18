/**
 * herdr keybinding syntax: capture, normalization, validation, conflicts.
 *
 * Kept free of DOM/Tauri except for the `KeyboardEvent` shape so the rules can
 * be tested. The rules come from herdr's own documentation:
 *
 *   - `prefix+n` needs the prefix; `ctrl+alt+n` is a direct terminal shortcut.
 *   - Accepted: plain keys, ctrl/shift/alt/cmd/super, enter/tab/esc/arrows,
 *     and named punctuation (minus, comma, ampersand, plus, backtick).
 *   - Navigate mode takes plain keys but not prefix+, esc, enter, tab,
 *     shift+tab, left, right, or an unmodified 1-9.
 *   - Most reliable direct bindings are ctrl+letter, function keys and
 *     explicit modified chords; alt, cmd/super and punctuation-with-modifiers
 *     depend on the outer terminal.
 */

export type Kind = "prefix" | "action" | "navigate" | "indexed" | "command";
export type Platform = "mac" | "other";
export type Mod = "ctrl" | "shift" | "alt" | "cmd";

/**
 * Emission order. herdr's own docs are inconsistent about it
 * (`ctrl+shift+alt+left` but also `alt+shift+left`), so we pick one order for
 * output and compare order-insensitively everywhere else.
 */
const MOD_ORDER: Mod[] = ["ctrl", "shift", "alt", "cmd"];

const MOD_ALIASES: Record<string, Mod> = {
  ctrl: "ctrl",
  control: "ctrl",
  shift: "shift",
  alt: "alt",
  option: "alt",
  opt: "alt",
  cmd: "cmd",
  command: "cmd",
  super: "cmd",
  win: "cmd",
  meta: "cmd",
};

/** Punctuation herdr gives a name to. */
const PUNCT_NAMES: Record<string, string> = {
  "-": "minus",
  ",": "comma",
  "&": "ampersand",
  "+": "plus",
  "`": "backtick",
};

const KEY_ALIASES: Record<string, string> = {
  escape: "esc",
  return: "enter",
  arrowleft: "left",
  arrowright: "right",
  arrowup: "up",
  arrowdown: "down",
  spacebar: "space",
};

export const RANGE = "1..9";

export type Parsed = {
  prefix: boolean;
  mods: Mod[];
  /** Normalized key name, or `1..9` for the indexed range. */
  key: string;
  range: boolean;
};

const sortMods = (mods: Iterable<Mod>): Mod[] => {
  const set = new Set(mods);
  return MOD_ORDER.filter((m) => set.has(m));
};

function normalizeKey(raw: string): string {
  if (raw === " ") return "space";
  const k = raw.toLowerCase();
  if (k === RANGE) return RANGE;
  if (KEY_ALIASES[k]) return KEY_ALIASES[k];
  if (PUNCT_NAMES[k]) return PUNCT_NAMES[k];
  return k;
}

/** Parse a herdr binding string. Returns null for an empty or malformed one. */
export function parse(text: string): Parsed | null {
  const t = text.trim();
  if (t === "") return null;

  // A trailing `+` can only be the plus key; herdr's own name for it is `plus`.
  const raw = t.endsWith("+") ? `${t.slice(0, -1)}+plus` : t;
  const tokens = raw.split("+").filter((x) => x !== "");
  if (!tokens.length) return null;

  let prefix = false;
  const mods = new Set<Mod>();
  const keys: string[] = [];

  for (const token of tokens) {
    const lower = token.toLowerCase();
    if (lower === "prefix") {
      prefix = true;
    } else if (MOD_ALIASES[lower]) {
      mods.add(MOD_ALIASES[lower]);
    } else {
      keys.push(normalizeKey(token));
    }
  }

  // Modifier-only values are how `[keys.indexed]` drives 1..9.
  if (keys.length === 0) {
    return prefix || mods.size
      ? { prefix, mods: sortMods(mods), key: RANGE, range: true }
      : null;
  }
  if (keys.length > 1) return null; // two non-modifier tokens is not a chord

  const key = keys[0];
  return { prefix, mods: sortMods(mods), key, range: key === RANGE };
}

export function canonical(p: Parsed): string {
  return [...(p.prefix ? ["prefix"] : []), ...p.mods, p.key].join("+");
}

/** Chords a binding actually occupies; a range covers nine of them. */
export function expand(p: Parsed): string[] {
  if (!p.range) return [canonical(p)];
  return Array.from({ length: 9 }, (_, i) =>
    canonical({ ...p, key: String(i + 1), range: false })
  );
}

const isLetter = (k: string) => /^[a-z]$/.test(k);
const isDigit = (k: string) => /^[0-9]$/.test(k);
const isFn = (k: string) => /^f([1-9]|1[0-9]|2[0-4])$/.test(k);
const NAMED = new Set(["enter", "tab", "esc", "left", "right", "up", "down", "space"]);
const isNamedPunct = (k: string) => Object.values(PUNCT_NAMES).includes(k);

// --- validation ------------------------------------------------------------

/** Rules herdr documents as prohibited. An empty array means acceptable. */
export function validate(text: string, kind: Kind, acceptsRange = false): string[] {
  const t = text.trim();
  if (t === "") return []; // unset / explicitly disabled

  const p = parse(t);
  if (!p) return [`"${t}" はキー構文として解釈できません`];

  const errors: string[] = [];
  if (p.range && !acceptsRange) {
    errors.push("この項目は 1..9 のレンジ表記を受け付けません");
  }

  switch (kind) {
    case "prefix":
      if (p.prefix) errors.push("prefix キー自体に prefix+ は付けられません");
      if (p.range) errors.push("prefix キーにレンジ表記は使えません");
      break;

    case "navigate":
      // Consumed by navigate mode's own modal loop.
      if (p.prefix) errors.push("navigate モードのキーに prefix+ は使えません");
      if (["esc", "enter", "tab"].includes(p.key))
        errors.push(`navigate モードでは ${p.key} は予約されています`);
      if (["left", "right"].includes(p.key))
        errors.push(`${p.key} 矢印は常に左右のペイン移動に割り当てられています`);
      if (isDigit(p.key) && p.mods.length === 0)
        errors.push("navigate モードでは修飾なしの 1〜9 は予約されています");
      break;

    case "indexed":
      if (p.prefix) errors.push("[keys.indexed] は修飾キーのみを指定します（prefix+ は不可）");
      if (!p.range) errors.push('[keys.indexed] は "ctrl" のように修飾キーのみを指定します');
      break;

    case "action":
    case "command":
      if (!p.prefix && p.mods.length === 0 && !isFn(p.key) && !NAMED.has(p.key))
        errors.push("修飾キーなしの直接バインドは通常の入力を奪うため使えません。prefix+ を付けてください");
      break;
  }
  return errors;
}

// --- terminal reliability --------------------------------------------------

export type Risk = { level: "safe" | "caution" | "risky"; reason: string };

/**
 * How likely the outer terminal is to actually deliver this chord. Prefix-mode
 * bindings are read by herdr itself once prefix mode is open, so they are
 * reliable regardless of the chord.
 */
export function risk(text: string, kind: Kind): Risk {
  const t = text.trim();
  if (t === "") return { level: "safe", reason: "未設定" };
  const p = parse(t);
  if (!p) return { level: "risky", reason: "解釈できない構文" };

  if (kind === "navigate")
    return { level: "safe", reason: "navigate モード中のみ有効なローカルキー" };
  if (kind === "indexed")
    return p.mods.includes("alt") || p.mods.includes("cmd")
      ? { level: "risky", reason: "alt / cmd は端末が横取りすることがあります" }
      : { level: "safe", reason: "修飾キー + 1〜9" };
  if (p.prefix) return { level: "safe", reason: "prefix モード中に herdr が直接読み取ります" };

  if (p.mods.includes("cmd"))
    return { level: "risky", reason: "cmd / super は端末やOSに奪われることが多い" };
  if (p.mods.includes("alt"))
    return { level: "risky", reason: "alt は端末や tmux の設定次第で届きません" };
  if (isFn(p.key)) return { level: "safe", reason: "ファンクションキーは直接バインドでも安定" };
  if (p.mods.length === 1 && p.mods[0] === "ctrl" && isLetter(p.key))
    return { level: "safe", reason: "ctrl + 英字は直接バインドで最も安定" };
  if (isNamedPunct(p.key) || (!isLetter(p.key) && !isDigit(p.key) && !NAMED.has(p.key)))
    return { level: "risky", reason: "修飾キー付きの記号は端末依存です" };
  if (p.mods.length === 0)
    return { level: "risky", reason: "修飾キーなしの直接バインドは通常の入力を奪います" };
  return { level: "caution", reason: "明示的な修飾チョード。端末で実際に届くか確認してください" };
}

// --- conflicts -------------------------------------------------------------

/** Navigate-mode keys live in their own modal scope. */
export const scopeOf = (kind: Kind): "global" | "navigate" =>
  kind === "navigate" ? "navigate" : "global";

export type Entry = { path: string; value: string; kind: Kind };
export type Conflict = { chord: string; scope: "global" | "navigate"; paths: string[] };

/**
 * Bindings occupying the same chord in the same scope. Ranges are expanded, so
 * `prefix+1..9` collides with a hand-written `prefix+3`.
 */
export function conflicts(entries: Entry[]): Conflict[] {
  const seen = new Map<string, Conflict>();

  for (const e of entries) {
    const p = parse(e.value);
    if (!p) continue;
    const scope = scopeOf(e.kind);
    for (const chord of expand(p)) {
      const id = `${scope}::${chord}`;
      const hit = seen.get(id);
      if (hit) hit.paths.push(e.path);
      else seen.set(id, { chord, scope, paths: [e.path] });
    }
  }

  return [...seen.values()]
    .filter((g) => g.paths.length > 1)
    .sort((a, b) => a.chord.localeCompare(b.chord));
}

// --- capture ---------------------------------------------------------------

/** The bits of a KeyboardEvent we need, so tests need no DOM. */
export type KeyEventLike = {
  key: string;
  code: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
};

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "CapsLock", "Dead"]);

/**
 * Turn a keypress into herdr syntax, without any `prefix+`.
 * Returns null while only modifiers are held, so a capture UI keeps waiting.
 */
export function fromEvent(ev: KeyEventLike, platform: Platform = "other"): string | null {
  if (MODIFIER_KEYS.has(ev.key)) return null;

  const mods = new Set<Mod>();
  if (ev.ctrlKey) mods.add("ctrl");
  if (ev.altKey) mods.add("alt");
  if (ev.metaKey) mods.add("cmd");

  let key: string;
  let shiftIsInherent = false;

  const printable = ev.key.length === 1 ? ev.key : "";
  // A symbol the layout produced. `alt` is excluded because macOS composes
  // characters with it (alt+a reports "a-ring"), which is not a symbol key.
  const symbol = printable !== "" && !/^[a-z0-9]$/i.test(printable) && !ev.altKey;

  if (symbol) {
    // The terminal reports the character itself, so shift is already baked
    // into it: shift+7 arrives as "&", which herdr names `ampersand`.
    // Reporting shift as well would describe a chord no terminal sends.
    key = normalizeKey(printable);
    shiftIsInherent = true;
  } else if (/^Key[A-Z]$/.test(ev.code)) {
    // ev.key is unusable for letters: shift uppercases it, and alt on macOS
    // replaces it with the composed character.
    key = ev.code.slice(3).toLowerCase();
  } else if (/^Digit[0-9]$/.test(ev.code)) {
    key = ev.code.slice(5);
  } else if (/^F\d+$/.test(ev.code)) {
    key = ev.code.toLowerCase();
  } else if (printable) {
    key = printable.toLowerCase();
  } else {
    key = normalizeKey(ev.key);
  }

  if (ev.shiftKey && !shiftIsInherent) mods.add("shift");

  const text = canonical({ prefix: false, mods: sortMods(mods), key, range: false });
  // `super` is the spelling used outside macOS.
  return platform === "mac" ? text : text.replace(/\bcmd\b/, "super");
}

/** Attach `prefix+` for items that need it. */
export const withPrefix = (chord: string): string =>
  chord.startsWith("prefix+") ? chord : `prefix+${chord}`;
