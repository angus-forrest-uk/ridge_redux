import { createResource, createSignal } from "solid-js";
import { fetchReadme } from "../lib/api.ts";

/* The README as plain text in a modal. Fetched the first time it opens. */
export default function ReadmeDialog(props: { ref: (api: { open: () => void }) => void }) {
  let dialog!: HTMLDialogElement;
  const [wanted, setWanted] = createSignal(false);
  const [readme] = createResource(wanted, fetchReadme);

  props.ref({
    open() {
      setWanted(true);
      dialog.showModal();
    },
  });

  return (
    <dialog id="readme" ref={dialog}>
      <form method="dialog"><button>close</button></form>
      <pre id="readme-text">
        {readme.error ? `could not load the README (${readme.error.message})` : readme() ?? "loading…"}
      </pre>
    </dialog>
  );
}
