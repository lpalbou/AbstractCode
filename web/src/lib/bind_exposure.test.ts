import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { bindExposureWarning } from "../../bin/server.js";

// `--host` beyond loopback exposes the app to other machines: the start says
// so in words, before listening (the gateway already serves it at /apps/code/).

const CLI = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "bin", "cli.js");

describe("bind exposure warning", () => {
  it("is silent for loopback hosts", () => {
    for (const host of ["127.0.0.1", "127.0.0.2", "localhost", "::1", "[::1]"]) {
      expect(bindExposureWarning(host, 3002), host).toBeNull();
    }
  });

  it("warns for the wildcard and for any other address", () => {
    for (const host of ["0.0.0.0", "::", "[::]"]) {
      const w = bindExposureWarning(host, 3002);
      expect(w, host).toContain(`--host ${host} exposes AbstractCode Web beyond this machine`);
      expect(w).toContain("every network interface");
      expect(w).toContain("port 3002");
    }
    expect(bindExposureWarning("192.168.1.20", 4000)).toContain("it listens on 192.168.1.20");
    expect(bindExposureWarning("127.evil.example", 4000)).toContain("beyond this machine");
  });

  it("the CLI prints it at start, before the bind", async () => {
    // 192.0.2.1 is TEST-NET-1 (RFC 5737): never assigned here, so the bind
    // fails right after the warning and nothing is ever exposed by this test.
    const child = spawn(process.execPath, [CLI, "--host", "192.0.2.1", "--port", "3999", "--gateway-url", "http://127.0.0.1:1"], {
      env: { PATH: process.env.PATH || "", HOME: process.env.HOME || "" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stderr = "";
    child.stderr.on("data", (c) => (stderr += String(c)));
    const code = await new Promise<number | null>((resolve) => {
      const timer = setTimeout(() => child.kill("SIGKILL"), 15000);
      child.on("exit", (c) => {
        clearTimeout(timer);
        resolve(c);
      });
    });
    expect(stderr).toContain("WARNING: --host 192.0.2.1 exposes AbstractCode Web beyond this machine");
    expect(code).not.toBe(0);
  }, 20000);
});
