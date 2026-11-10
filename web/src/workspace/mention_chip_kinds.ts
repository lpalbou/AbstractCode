// The kinds of the `@` chips the shared composer holds ("entity", "agent"), for the line above it
// (`mentions.ts` agentHint, operator 2026-11-10). The kit's WorkflowChat keeps its chips to itself
// (panel-chat 0.9.11 exposes no chip callback), so Code reads the chips it renders: each one is a
// `.pc-mention-chip__name` carrying `data-mention-kind`; since panel-chat 0.9.12 the chips sit
// inside the text (drawn by the composer's mirror over its textarea), with that same markup.
// agent_mention.test.tsx pins it on the vendored kit, so a kit that changes it turns the test red,
// never the line silent.
import { useEffect, useState } from "react";

/** The composer's chip names, one per chip, with their kind. */
export const MENTION_CHIP_SELECTOR = ".pc-mention-chip__name[data-mention-kind]";

/** The kinds of the chips under `root`, in order; [] without a root. */
export function chipKindsIn(root: Pick<ParentNode, "querySelectorAll"> | null | undefined): string[] {
  if (!root) return [];
  return Array.from(root.querySelectorAll(MENTION_CHIP_SELECTOR), (el) => String(el.getAttribute("data-mention-kind") || ""));
}

function sameKinds(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((kind, i) => kind === b[i]);
}

/** The kinds of the chips the composer under `element` holds (the conversation's element, from a
 * callback ref: it is mounted again after an automation's view), kept current as chips come and go. */
export function useComposerChipKinds(element: HTMLElement | null): string[] {
  const [kinds, setKinds] = useState<string[]>([]);
  useEffect(() => {
    if (!element) {
      setKinds((now) => (now.length ? [] : now));
      return;
    }
    const read = () => setKinds((now) => {
      const next = chipKindsIn(element);
      return sameKinds(now, next) ? now : next;
    });
    read();
    const observer = new MutationObserver(read);
    observer.observe(element, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-mention-kind"] });
    return () => observer.disconnect();
  }, [element]);
  return kinds;
}
