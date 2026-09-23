import { describe, it, expect } from "vitest";
import { normalizeWorkflowCatalog, publishedWorkflowChoices, resolveRestoredWorkflow, workflowChoiceLabel } from "./catalog";

const bundle = (version: string, extra: Record<string, unknown> = {}) => ({
  bundle_id: "coding", bundle_version: version, status: "published", actions: { can_run: true },
  entrypoints: [{ flow_id: "coding", name: "Coding" }], ...extra,
});
describe("published workflow picker projection", () => {
  it("uses catalog default, hides installed duplicates/older releases without destroying pinned identities", () => {
    const full = normalizeWorkflowCatalog({ default_bundle_id: "coding", items: [bundle("2.0")] }, { items: [bundle("1.0", { is_default: true }), bundle("2.0")] });
    const choices = publishedWorkflowChoices(full);
    expect(choices).toHaveLength(1);
    expect(choices[0]).toMatchObject({ bundleVersion: "1.0", registryScope: "tenant_catalog", isDefault: true });
    expect(resolveRestoredWorkflow(full, { runWorkflowId: "coding@2.0:coding" })?.registryScope).toBe("private");
    expect(resolveRestoredWorkflow(full, { inputData: { workflow_selection: { registry_scope: "tenant_catalog", bundle_id: "coding", bundle_version: "2.0", flow_id: "coding" } } })?.bundleVersion).toBe("2.0");
  });
  it("selects version before entrypoints so removed flows are not resurrected", () => {
    const full = normalizeWorkflowCatalog(undefined, { items: [bundle("1.0", { entrypoints: [{ flow_id: "removed" }] }), bundle("2.0", { is_default: true })] });
    expect(publishedWorkflowChoices(full).map(row => row.flowId)).toEqual(["coding"]);
    expect(full.every(row => !row.isDefault)).toBe(true); // Bundle default is not app default.
  });
  it("excludes drafts, blocked, deprecated and denied; uses highest visible release if default is inaccessible", () => {
    const full = normalizeWorkflowCatalog({ items: [bundle("0.1-draft.1"), bundle("0.1", { is_draft: true }), bundle("0.2", { is_published: false })] }, { items: [bundle("1.2"), bundle("1.10"), bundle("2", { status: "blocked" }), bundle("3", { status: "deprecated" }), bundle("4", { is_default: true, actions: { can_run: false } })] });
    expect(publishedWorkflowChoices(full).map(row => row.bundleVersion)).toEqual(["1.10"]);
  });
  it("keeps distinct same-name bundles and labels their actual origin, not shared/private", () => {
    const choices = publishedWorkflowChoices(normalizeWorkflowCatalog({ items: [bundle("1"), bundle("1", { bundle_id: "another" })] }));
    expect(choices).toHaveLength(2);
    expect(choices.map(row => workflowChoiceLabel(row, choices)).sort()).toEqual(["Coding · another", "Coding · coding"]);
  });
});
