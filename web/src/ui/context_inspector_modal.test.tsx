import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { ContextInspectorModal } from "./app";

describe("ContextInspectorModal", () => {
  it("renders a mobile-safe close action inside the modal body", () => {
    const html = renderToStaticMarkup(
      <ContextInspectorModal gateway={{} as any} root_run_id="run_root" inspect_run_id="run_root" on_close={() => {}} />
    );
    expect(html).toContain("modal_body_actions");
    expect((html.match(/modal_close_btn/g) || []).length).toBeGreaterThanOrEqual(2);
  });
});

