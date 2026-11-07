import { useState } from "react";
import { useAvatarConversationPreferences } from "../vendor/abstractchat/conversation.js";

/** The Conversation | Avatar switch. Code always opens on Conversation (operator rule 2026-11-07):
 * the view is state of this visit only and is never read back from the browser. The shared hook
 * still keeps the person's attention cue preferences (approval, question, completion). */
export function useConversationView(scope: string) {
  const saved = useAvatarConversationPreferences(scope);
  const [mode, setMode] = useState<"chat" | "avatar">("chat");
  return { mode, setMode, preferences: saved.preferences, setPreferences: saved.setPreferences };
}
