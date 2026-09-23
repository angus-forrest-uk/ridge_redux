import { createSignal } from "solid-js";
import { configToText, parseConfig } from "../lib/params.ts";
import { useRidge } from "../state.ts";

/* Export the current configuration (one click to copy) or import one (one
 * click to paste from the clipboard, then apply). The textarea is the manual
 * fallback and shows exactly what is copied or parsed. */
export default function ConfigDialog(props: { ref: (api: { open: () => void }) => void }) {
  const { params, applyConfig } = useRidge();
  let dialog!: HTMLDialogElement;
  let box!: HTMLTextAreaElement;
  const [note, setNote] = createSignal("");

  props.ref({
    open() {
      box.value = configToText(params);
      setNote("");
      dialog.showModal();
      box.select();
    },
  });

  async function copy() {
    try {
      await navigator.clipboard.writeText(box.value);
      setNote("copied to clipboard");
    } catch {
      box.select();
      setNote("copy failed — copy the selected text by hand");
    }
  }

  async function paste() {
    try {
      box.value = await navigator.clipboard.readText();
      setNote("");
    } catch {
      setNote("couldn't read the clipboard — paste into the box");
    }
  }

  function apply() {
    const config = parseConfig(box.value);
    if (!config) {
      setNote("that doesn't look like a config");
      return;
    }
    applyConfig(config);
    dialog.close();
  }

  return (
    <dialog id="config" ref={dialog} aria-label="configuration">
      <form method="dialog"><button>close</button></form>
      <div id="config-body">
        <p>Copy this view to share it, or paste a configuration to load one.</p>
        <textarea id="config-text" ref={box} rows={12} spellcheck={false} />
        <div id="config-actions">
          <button type="button" id="config-copy" onClick={copy}>copy</button>
          <button type="button" id="config-paste" onClick={paste}>paste from clipboard</button>
          <button type="button" id="config-apply" onClick={apply}>apply</button>
          <span id="config-note">{note()}</span>
        </div>
      </div>
    </dialog>
  );
}
