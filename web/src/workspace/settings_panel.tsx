import React from "react";
import {
  AfDrawer,
  ProviderModelPicker,
  ToolPolicyEditor,
  type ToolPolicySelection,
  type SpeculationValue,
} from "@abstractframework/ui-kit";
import type { ToolSpec, WorkspacePolicy } from "./catalog";
import { gateway } from "./transport";
import { SkillsPicker } from "./skills_picker";
import { navigateTabs } from "./tabs";
import { modelDiscovery } from "./model_discovery";
import { resolveToolPermissions, type PermissionLevel } from "./tool_permissions";

export type RunPreferences = {
  provider: string;
  model: string;
  reasoning: string;
  speculation?: SpeculationValue;
  maxIterations: string;
  maxTokens: string;
  system: string;
  workspaceRoot: string;
  workspaceMode: string;
  allowedPaths: string;
  tools: ToolPolicySelection;
  toolsCustomized: boolean;
  permissions: PermissionLevel;
  skills: string[];
  /** Workflow for new conversations: "@default" (the gateway's default agent
   * workflow, resolved by the server at run start) or a catalog workflow id. */
  workflow: string;
  /** List non-agent workflows in the selector too. */
  showAllWorkflows: boolean;
};
export const DEFAULT_PREFERENCES: RunPreferences = {
  provider: "",
  model: "",
  reasoning: "",
  maxIterations: "",
  maxTokens: "",
  system: "",
  workspaceRoot: "",
  workspaceMode: "",
  allowedPaths: "",
  tools: { mode: "all", selected: [], approval: {} },
  toolsCustomized: false,
  permissions: "default",
  skills: [],
  workflow: "@default",
  showAllWorkflows: false,
};
export type SettingsTab = "model" | "workspace" | "tools" | "skills";

export function SettingsPanel({
  open,
  onClose,
  tab,
  onTab,
  value,
  onChange,
  policy,
  tools,
  disabled,
  defaultModel,
  workflowDefault,
}: {
  open: boolean;
  onClose: () => void;
  tab: SettingsTab;
  onTab: (tab: SettingsTab) => void;
  value: RunPreferences;
  onChange: (value: RunPreferences) => void;
  policy: WorkspacePolicy | null;
  tools: ToolSpec[];
  disabled: boolean;
  defaultModel?: { provider: string; model: string };
  workflowDefault?: boolean;
}) {
  const update = (patch: Partial<RunPreferences>) =>
    onChange({ ...value, ...patch });
  return (
    <AfDrawer
      open={open}
      onClose={onClose}
      label="Run settings"
      title="Run settings"
      width={480}
      topOffset={60}
      className="code-settings-drawer"
    >
      <div className="code-settings">
        <p className="code-muted">
          Settings apply to the next turn. Your gateway enforces the available
          permissions.
        </p>
        <div
          className="code-settings-tabs"
          role="tablist"
          aria-label="Settings sections"
          onKeyDown={navigateTabs}
        >
          {(["model", "workspace", "tools", "skills"] as const).map((name) => (
            <button
              role="tab"
              key={name}
              tabIndex={tab === name ? 0 : -1}
              aria-selected={tab === name}
              onClick={() => onTab(name)}
            >
              {name === "model"
                ? "Model & behavior"
                : name === "workspace"
                  ? "Workspace"
                  : name === "tools"
                    ? "Tools"
                    : "Skills"}
            </button>
          ))}
        </div>
        {disabled ? (
          <p className="code-notice">
            Settings are unavailable while disconnected or while a run is
            active. Reconnect, or finish or stop the run, then try again.
          </p>
        ) : null}
        {tab === "model" ? (
          <>
            <section className="code-settings-section">
              <h3>Model</h3>
              <ProviderModelPicker
                defaultModeLabel={
                  workflowDefault ? "Workflow default" : "Gateway default"
                }
                value={value}
                onChange={update}
                disabled={disabled}
                effectiveDefault={defaultModel}
                fetchModelCapabilities={modelDiscovery.fetchModelCapabilities}
                enableSpeculation
                defaultHint={
                  defaultModel
                    ? `${workflowDefault ? "Workflow" : "Gateway"} default: ${defaultModel.provider} · ${defaultModel.model}.`
                    : undefined
                }
                fetchProviders={async () => {
                  const data = await gateway.discovery_providers();
                  return Array.isArray(data.providers)
                    ? data.providers
                    : Array.isArray(data.items)
                      ? data.items
                      : [];
                }}
                fetchModels={async (provider) => {
                  const data =
                    await gateway.discovery_provider_models(provider);
                  return (
                    Array.isArray(data.models)
                      ? data.models
                      : Array.isArray(data.items)
                        ? data.items
                        : []
                  ).map((model: any) =>
                    typeof model === "string" ? model : model.id || model.name,
                  );
                }}
              />
            </section>
            <section className="code-settings-section">
              <h3>Behavior</h3>
              <div className="code-field-pair">
                <label className="code-field">
                  Iteration limit
                  <input
                    type="number"
                    min={1}
                    step={1}
                    placeholder="Workflow default"
                    value={value.maxIterations}
                    disabled={disabled}
                    onChange={(e) => update({ maxIterations: e.target.value })}
                  />
                </label>
                <label className="code-field">
                  Context token limit
                  <input
                    type="number"
                    min={1}
                    step={1}
                    placeholder="Gateway default"
                    value={value.maxTokens}
                    disabled={disabled}
                    onChange={(e) => update({ maxTokens: e.target.value })}
                  />
                </label>
              </div>
              <label className="code-field">
                Additional instructions
                <textarea
                  rows={5}
                  value={value.system}
                  placeholder="Project conventions or guidance for this conversation…"
                  disabled={disabled}
                  onChange={(e) => update({ system: e.target.value })}
                />
              </label>
              <p className="code-field-help">
                Reasoning and additional instructions depend on the selected
                workflow and model.
              </p>
            </section>
          </>
        ) : tab === "workspace" ? (
          <section className="code-settings-section">
            <h3>Workspace access</h3>
            <div className="code-policy">
              <strong>
                {policy?.clientWorkspaceScopeOverrides
                  ? "Client scope requests enabled"
                  : "Managed by your gateway"}
              </strong>
              <p>
                {policy?.clientWorkspaceScopeOverrides
                  ? "You may request a workspace scope. Gateway policy remains authoritative."
                  : "The gateway chooses and restricts the workspace. File browsing and tool execution follow the same policy."}
              </p>
            </div>
            <dl className="code-facts">
              <dt>Allowed access modes</dt>
              <dd>
                {policy?.allowedAccessModes.join(", ") || "Gateway default"}
              </dd>
              <dt>Available mounts</dt>
              <dd>
                {policy?.mounts.length
                  ? policy.mounts.map((mount) => mount.label).join(", ")
                  : "Default workspace"}
              </dd>
            </dl>
            {policy?.clientWorkspaceScopeOverrides ? (
              <>
                <label className="code-field">
                  Workspace root
                  <input
                    value={value.workspaceRoot}
                    placeholder="Gateway default"
                    disabled={disabled}
                    onChange={(e) => update({ workspaceRoot: e.target.value })}
                  />
                </label>
                <label className="code-field">
                  Access mode
                  <select
                    value={value.workspaceMode}
                    disabled={disabled}
                    onChange={(e) => update({ workspaceMode: e.target.value })}
                  >
                    <option value="">Gateway default</option>
                    {policy.allowedAccessModes.map((mode) => (
                      <option key={mode} value={mode}>
                        {mode.replace(/_/g, " ")}
                      </option>
                    ))}
                  </select>
                </label>
                {value.workspaceMode === "workspace_or_allowed" ? (
                  <label className="code-field">
                    Additional allowed paths
                    <textarea
                      rows={3}
                      value={value.allowedPaths}
                      disabled={disabled}
                      placeholder="One path per line"
                      onChange={(e) => update({ allowedPaths: e.target.value })}
                    />
                  </label>
                ) : null}
              </>
            ) : null}
          </section>
        ) : tab === "tools" ? (
          <section className="code-settings-section">
            <label className="code-field">
              Permissions
              <select aria-label="Permissions" value={value.permissions} disabled={disabled} onChange={event => update({ permissions: event.target.value as PermissionLevel })}>
                <option value="default">Workflow / Gateway defaults</option>
                <option value="read">Read</option>
                <option value="write">Write</option>
                <option value="all">All enabled tools</option>
              </select>
            </label>
            <p className="code-field-help">Saved for this Gateway account and future turns. Permissions never enable an unchecked tool. Explicit Ask overrides still ask, even with permissions: all.</p>
            <ToolPolicyEditor
              tools={tools
                .filter((tool) => tool.enabled)
                .map((tool) => ({
                  ...tool,
                  default_approval:
                    resolveToolPermissions(tools, value.tools, value.permissions).autoApproveTools.includes(tool.name)
                      ? "approve"
                      : "ask",
                }))}
              value={value.tools}
              onChange={(next) =>
                update({ tools: next, toolsCustomized: true })
              }
              disabled={disabled}
              subtitle="Choose available tools and when they should ask you. Gateway restrictions always apply."
              note="An empty custom selection disables every tool. Approvals follow the permissions level and each tool's override."
            />
            {tools.some((tool) => !tool.enabled) ? (
              <details className="code-disabled-tools">
                <summary>
                  {tools.filter((tool) => !tool.enabled).length} tools
                  unavailable under gateway policy
                </summary>
                {tools
                  .filter((tool) => !tool.enabled)
                  .map((tool) => (
                    <p key={tool.name}>
                      <strong>{tool.name}</strong>
                      <br />
                      {tool.whyDisabled || "Disabled by the gateway"}
                    </p>
                  ))}
              </details>
            ) : null}
            <button
              className="code-text-button"
              disabled={disabled}
              onClick={() =>
                update({
                  toolsCustomized: false,
                  tools: DEFAULT_PREFERENCES.tools,
                  permissions: "default",
                })
              }
            >
              Use workflow tool defaults
            </button>
          </section>
        ) : (
          <SkillsPicker
            value={value.skills}
            onChange={(skills) => update({ skills })}
            disabled={disabled}
          />
        )}
      </div>
    </AfDrawer>
  );
}
