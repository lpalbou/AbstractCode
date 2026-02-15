function weak_hash_hex32(text: string): string {
  // Fallback only. This should match nothing server-side; it's used only when SubtleCrypto is missing.
  let h = 5381;
  for (let i = 0; i < text.length; i++) h = (h * 33) ^ text.charCodeAt(i);
  const u = h >>> 0;
  return u.toString(16).padStart(8, "0").repeat(8).slice(0, 32);
}

async function sha256_hex(text: string): Promise<string> {
  try {
    const c: any = globalThis as any;
    const subtle = c?.crypto?.subtle;
    if (!subtle || typeof subtle.digest !== "function") throw new Error("subtle crypto unavailable");
    const enc = new TextEncoder().encode(text);
    const buf = await subtle.digest("SHA-256", enc);
    const bytes = new Uint8Array(buf);
    let out = "";
    for (const b of bytes) out += b.toString(16).padStart(2, "0");
    return out;
  } catch {
    return weak_hash_hex32(text);
  }
}

const SAFE_RUN_ID_RE = /^[a-zA-Z0-9_-]+$/;

export async function session_memory_owner_run_id(session_id: string): Promise<string> {
  const sid = String(session_id || "").trim();
  if (!sid) throw new Error("session_memory_owner_run_id: session_id is required");
  if (SAFE_RUN_ID_RE.test(sid)) {
    const rid = `session_memory_${sid}`;
    if (SAFE_RUN_ID_RE.test(rid)) return rid;
  }
  // Match gateway's fallback shape: session_memory_sha_<sha32>.
  const digest = (await sha256_hex(sid)).slice(0, 32);
  return `session_memory_sha_${digest}`;
}
