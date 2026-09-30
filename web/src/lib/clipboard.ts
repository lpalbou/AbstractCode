import { clipboardWrite } from "./secure-context";

/** Copy text; true only when it reached the clipboard (plain http uses the execCommand fallback). */
export async function copy_text(text: string): Promise<boolean> {
  const t = String(text ?? "");
  if (!t) return false;
  return clipboardWrite(t);
}
