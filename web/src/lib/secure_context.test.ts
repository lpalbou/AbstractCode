// Plain http from another machine (LAN, Tailscale) is not a secure context:
// crypto.randomUUID and navigator.clipboard are missing there. These tests run
// the app's id and copy paths with both removed.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { newId } from "../workspace/transport";
import { random_id } from "./ids";
import { copy_text } from "./clipboard";

const V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const realCrypto = Object.getOwnPropertyDescriptor(globalThis, "crypto");
const realNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
const realDocument = Object.getOwnPropertyDescriptor(globalThis, "document");
const define = (name: string, value: unknown) =>
  Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
const restore = (name: string, desc: PropertyDescriptor | undefined) => {
  if (desc) Object.defineProperty(globalThis, name, desc);
  else delete (globalThis as Record<string, unknown>)[name];
};

describe("non-secure context (crypto.randomUUID undefined)", () => {
  beforeEach(() => {
    const c = globalThis.crypto;
    define("crypto", { getRandomValues: <T extends ArrayBufferView>(a: T) => c.getRandomValues(a as never) as T });
  });
  afterEach(() => restore("crypto", realCrypto));

  it("newId() (the workspace session id, minted in the first render) is a v4 UUID, unique over 10k", () => {
    expect((globalThis.crypto as { randomUUID?: unknown }).randomUUID).toBeUndefined();
    const ids = Array.from({ length: 10_000 }, () => newId());
    expect(ids.every((id) => V4.test(id))).toBe(true);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("random_id() (command/request ids, storage session ids) is a v4 UUID", () => {
    expect(V4.test(random_id())).toBe(true);
  });
});

describe("non-secure context (navigator.clipboard undefined)", () => {
  let copied: string[] = [];
  let execResult = true;
  beforeEach(() => {
    copied = [];
    define("navigator", {});
    const body = {
      nodes: [] as unknown[],
      appendChild(n: unknown) { this.nodes.push(n); },
      removeChild(n: unknown) { this.nodes = this.nodes.filter((x) => x !== n); },
    };
    let selected = "";
    define("document", {
      body,
      activeElement: null,
      createElement: () => {
        const el = {
          value: "",
          style: {} as Record<string, string>,
          setAttribute() {},
          select() { selected = el.value; },
          setSelectionRange() {},
        };
        return el;
      },
      execCommand: (cmd: string) => {
        if (cmd === "copy" && execResult) copied.push(selected);
        return cmd === "copy" && execResult;
      },
    });
  });
  afterEach(() => {
    restore("navigator", realNavigator);
    restore("document", realDocument);
  });

  it("copy_text falls back to execCommand('copy') and reports success", async () => {
    execResult = true;
    await expect(copy_text("token-123")).resolves.toBe(true);
    expect(copied).toEqual(["token-123"]);
  });

  it("copy_text reports failure when the fallback copy fails (the UI then says Copy failed)", async () => {
    execResult = false;
    await expect(copy_text("token-123")).resolves.toBe(false);
  });
});
