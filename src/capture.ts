/**
 * Modal key capture.
 *
 * Two phases, because Esc and Enter are themselves bindable:
 *   recording -> every keypress is swallowed and turned into a chord
 *   captured  -> Esc cancels, Enter confirms, or record again
 *
 * For a prefix-mode action the prefix key is pressed for real: press the
 * configured prefix (ctrl+a by default), the modal notices, and the next
 * keypress becomes `prefix+<chord>`.
 */
import {
  canonical,
  conflicts,
  fromEvent,
  parse,
  risk,
  validate,
  withPrefix,
  type Entry,
  type Kind,
  type Platform,
} from "./keys";

export const platform = (): Platform =>
  /Mac|iPhone|iPad/.test(navigator.userAgent) ? "mac" : "other";

const RISK_LABEL = { safe: "安定", caution: "要確認", risky: "端末依存" } as const;

export type CaptureRequest = {
  /** Dotted path, only used to exclude the row itself from conflict checks. */
  path: string;
  /** Human label shown in the dialog. */
  title: string;
  kind: Kind;
  acceptsRange: boolean;
  current: string;
  /** The prefix chord currently in effect, e.g. `ctrl+a`. */
  prefixChord: string;
  /** Every other binding, for live conflict detection. */
  others: Entry[];
};

const esc = (s: string) =>
  s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

function instructions(kind: Kind, prefixChord: string): string {
  switch (kind) {
    case "prefix":
      return "新しい prefix キーを押してください（prefix+ は付きません）";
    case "navigate":
      return "navigate モード中に使う単独キーを押してください（j / k / 矢印など）";
    case "indexed":
      return "修飾キーを押しながら 1〜9 のいずれかを押してください（修飾キーのみを使います）";
    default:
      return `キーを押してください。${
        prefixChord ? `先に prefix (${prefixChord}) を押すと prefix モードのバインドになります。` : ""
      }`;
  }
}

/** Resolves to the binding text to store, or null when cancelled. */
export function openCapture(req: CaptureRequest): Promise<string | null> {
  return new Promise((resolve) => {
    const plat = platform();
    const prefixParsed = parse(req.prefixChord);
    const prefixCanonical = prefixParsed ? canonical(prefixParsed) : "";

    const back = document.createElement("div");
    back.className = "capture-back";
    back.innerHTML = `
      <div class="capture" role="dialog" aria-modal="true" aria-label="キーの録音">
        <div class="cap-title">${esc(req.title)}</div>
        <div class="cap-hint">${esc(instructions(req.kind, req.kind === "action" || req.kind === "command" ? prefixCanonical : ""))}</div>
        <div class="cap-chord" id="cap-chord">キー入力待ち…</div>
        <div class="cap-notes" id="cap-notes"></div>
        <div class="cap-actions">
          <button id="cap-again">もう一度録音</button>
          <button id="cap-clear">無効にする (空)</button>
          <span class="cap-spacer"></span>
          <button id="cap-cancel">キャンセル (Esc)</button>
          <button id="cap-ok" class="primary" disabled>確定 (Enter)</button>
        </div>
      </div>`;
    document.body.appendChild(back);

    const chordEl = back.querySelector<HTMLElement>("#cap-chord")!;
    const notesEl = back.querySelector<HTMLElement>("#cap-notes")!;
    const okBtn = back.querySelector<HTMLButtonElement>("#cap-ok")!;

    /** Whether the prefix key has been pressed and we await the action key. */
    let prefixArmed = false;
    let recording = true;
    let result: string | null = null;

    const close = (value: string | null) => {
      window.removeEventListener("keydown", onKey, true);
      back.remove();
      resolve(value);
    };

    const show = (binding: string) => {
      result = binding;
      const errors = validate(binding, req.kind, req.acceptsRange);
      const r = risk(binding, req.kind);
      const clash = conflicts([...req.others, { path: req.path, value: binding, kind: req.kind }]).filter(
        (c) => c.paths.includes(req.path)
      );

      chordEl.textContent = binding || "(空)";
      chordEl.className = `cap-chord ${errors.length ? "bad" : r.level}`;

      const notes: string[] = [];
      for (const e of errors) notes.push(`<div class="cap-note err">${esc(e)}</div>`);
      notes.push(
        `<div class="cap-note ${r.level}"><b>${RISK_LABEL[r.level]}</b> ${esc(r.reason)}</div>`
      );
      for (const c of clash) {
        const others = c.paths.filter((p) => p !== req.path);
        notes.push(
          `<div class="cap-note warn"><b>衝突</b> ${esc(c.chord)} は ${others
            .map((p) => esc(p))
            .join(", ")} と重複します</div>`
        );
      }
      notesEl.innerHTML = notes.join("");
      okBtn.disabled = errors.length > 0;
    };

    const startRecording = () => {
      recording = true;
      prefixArmed = false;
      result = null;
      chordEl.textContent = "キー入力待ち…";
      chordEl.className = "cap-chord";
      notesEl.innerHTML = "";
      okBtn.disabled = true;
    };

    function onKey(ev: KeyboardEvent) {
      if (!recording) {
        if (ev.key === "Escape") {
          ev.preventDefault();
          close(null);
        } else if (ev.key === "Enter" && !okBtn.disabled) {
          ev.preventDefault();
          close(result);
        }
        return;
      }

      // While recording, nothing else may see the keypress.
      ev.preventDefault();
      ev.stopPropagation();

      const chord = fromEvent(ev, plat);
      if (chord === null) return; // modifiers only: keep waiting

      if (req.kind === "indexed") {
        // Only the modifier part is meaningful here.
        const p = parse(chord);
        const mods = p ? p.mods : [];
        if (!mods.length) {
          notesEl.innerHTML = `<div class="cap-note err">修飾キーを押しながら押してください</div>`;
          return;
        }
        recording = false;
        show(mods.join("+"));
        return;
      }

      const usesPrefix = req.kind === "action" || req.kind === "command";
      if (usesPrefix && !prefixArmed && prefixCanonical && chord === prefixCanonical) {
        // The prefix key itself: arm it and wait for the action key.
        prefixArmed = true;
        chordEl.textContent = `${chord} + …`;
        chordEl.className = "cap-chord armed";
        notesEl.innerHTML = `<div class="cap-note">prefix モードです。続けてキーを押してください。</div>`;
        return;
      }

      recording = false;
      show(prefixArmed ? withPrefix(chord) : chord);
    }

    window.addEventListener("keydown", onKey, true);
    back.querySelector<HTMLButtonElement>("#cap-again")!.onclick = startRecording;
    back.querySelector<HTMLButtonElement>("#cap-clear")!.onclick = () => close("");
    back.querySelector<HTMLButtonElement>("#cap-cancel")!.onclick = () => close(null);
    okBtn.onclick = () => close(result);
    back.onclick = (ev) => {
      if (ev.target === back) close(null);
    };

    if (req.current) {
      // Show what is currently bound until the first keypress lands.
      recording = false;
      show(req.current);
      recording = true;
      chordEl.textContent = `現在: ${req.current}`;
      chordEl.className = "cap-chord current";
      okBtn.disabled = true;
    }
  });
}
