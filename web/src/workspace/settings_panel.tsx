import React from "react";
import {
  ProviderModelPicker,
  ToolPolicyEditor,
  type ToolPolicySelection,
  type SpeculationValue,
  type WorkspaceRequest,
} from "@abstractframework/ui-kit";
import { CodeWorkspaceFolders, type RunWorkspace } from "./workspace_folders";
import type { ToolSpec } from "./catalog";
import { gateway } from "./transport";
import { SkillsPicker } from "./skills_picker";
import { modelDiscovery } from "./model_discovery";
import { resolveToolPermissions, type PermissionLevel } from "./tool_permissions";
import {
  gatewayDefaultStreamLabel,
  STREAMING_LOADING,
  type StreamRepliesMode,
  type StreamingCapability,
} from "./stream_replies";

/** The Run settings banner saying why settings are locked (switches point their reason at it). */
export const SETTINGS_LOCKED_ID = "code-settings-locked";

export type { RunWorkspace };

export type RunPreferences = {
  provider: string;
  model: string;
  reasoning: string;
  speculation?: SpeculationValue;
  maxIterations: string;
  maxTokens: string;
  system: string;
  /** An automation's workspaces (input_data.workspace, the run-level payload); null = "Use my default". Conversations keep null: their workspaces live on the session in the gateway. */
  workspace: RunWorkspace | null;
  tools: ToolPolicySelection;
  toolsCustomized: boolean;
  permissions: PermissionLevel;
  skills: string[];
  /** Workflow for new conversations: "@default" (the gateway's default agent
   * workflow, resolved by the server at run start) or a catalog workflow id. */
  workflow: string;
  /** "Stream replies": show the reply while the model writes it. */
  streamReplies: StreamRepliesMode;
};
export const DEFAULT_PREFERENCES: RunPreferences = {
  provider: "",
  model: "",
  reasoning: "",
  maxIterations: "",
  maxTokens: "",
  system: "",
  workspace: null,
  tools: { mode: "all", selected: [], approval: {} },
  toolsCustomized: false,
  permissions: "default",
  skills: [],
  workflow: "@default",
  streamReplies: "gateway_default",
};
/** The settings panels SettingsContent renders (Workflow and Voice have their own components). */
export type SettingsTab = "model" | "workspace" | "tools" | "skills";

/** One rail panel's preference sections (Model, Workspace, Tools or Skills). No drawer or navigation wrappers. */
export type SettingsContentProps = {
  tab: SettingsTab;
  value: RunPreferences;
  onChange: (value: RunPreferences) => void;
  tools: ToolSpec[];
  disabled: boolean;
  lockedReasonId?: string;
  defaultModel?: { provider: string; model: string };
  workflowDefault?: boolean;
  streaming?: StreamingCapability;
  /** An automation's folder: shown read-only instead of the root field (its runs always work there). */
  workspaceRootFixed?: string;
  /** Hide "Stream replies" (a conversation display choice; automations have no live view). */
  hideStreamReplies?: boolean;
  /** Replaces the locked notice (e.g. why an automation cannot be edited now). */
  lockedText?: string;
  /** Workspace tab: connected to a gateway (the workspaces load from it). */
  connected?: boolean;
  /** Workspace tab: an automation (its stored workspaces, run level) instead of the conversation's (session level). */
  automationFolders?: boolean;
  /** Workspace tab: the open conversation's session id (session level). */
  sessionId?: string;
  /** Workspace tab: opens "My default workspaces" (account level). */
  onOpenDefaultWorkspaces?: () => void;
  /** Workspace tab: bumped when the account default changed. */
  workspaceRefreshKey?: unknown;
  /** Workspace tab: injected gateway request (tests). */
  workspaceRequest?: WorkspaceRequest;
};

export function SettingsContent({
  tab, value, onChange, tools, disabled, defaultModel,
  workflowDefault, streaming = STREAMING_LOADING,
  lockedReasonId = SETTINGS_LOCKED_ID,
  workspaceRootFixed, hideStreamReplies, lockedText,
  connected = false, automationFolders = false, workspaceRequest,
  sessionId, onOpenDefaultWorkspaces, workspaceRefreshKey,
}: SettingsContentProps) {
  const update = (patch: Partial<RunPreferences>) => onChange({ ...value, ...patch });
  return <>
        {disabled ? (
          <p className="code-notice" id={lockedReasonId}>
            {lockedText ||
              "Settings are unavailable while disconnected or while a run is active. Reconnect, or finish or stop the run, then try again."}
          </p>
        ) : null}
        {tab === "model" ? (
          <>
            <section className="code-settings-section" aria-label="Model">
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
                inheritLabel="Gateway default"
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
              {hideStreamReplies ? null : (
                <StreamRepliesField
                  value={value.streamReplies}
                  onChange={(streamReplies) => update({ streamReplies })}
                  disabled={disabled}
                  streaming={streaming}
                />
              )}
            </section>
          </>
        ) : tab === "workspace" ? (
          <section className="code-settings-section">
            {workspaceRootFixed ? (
              <p className="code-field-help" data-setting="workspace-root-fixed">
                Runs work in the automation workspace <code title={workspaceRootFixed}>{workspaceRootFixed.split(/[\\/]/).filter(Boolean).pop() || workspaceRootFixed}</code>.
              </p>
            ) : null}
            <CodeWorkspaceFolders
              connected={connected}
              request={workspaceRequest}
              sessionId={sessionId}
              onOpenDefaults={onOpenDefaultWorkspaces}
              refreshKey={workspaceRefreshKey}
              automation={automationFolders ? { value: value.workspace, onChange: (workspace) => update({ workspace }) } : undefined}
            />
          </section>
        ) : tab === "tools" ? (
          <section className="code-settings-section">
            <label className="code-field">
              Permissions
              <select aria-label="Permissions" value={value.permissions} disabled={disabled} onChange={event => update({ permissions: event.target.value as PermissionLevel })}>
                <option value="default">Gateway default</option>
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
                  // The gateway's command-sandbox state of a process-spawning tool, on its card.
                  ...(tool.sandboxState ? { state: tool.sandboxState } : {}),
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
              Use gateway default
            </button>
          </section>
        ) : (
          <SkillsPicker
            value={value.skills}
            onChange={(skills) => update({ skills })}
            disabled={disabled}
            lockedReasonId={lockedReasonId}
          />
        )}
  </>;
}

/** "Stream replies": gateway default / on / off. Shown disabled, with the
 * reason, when the gateway does not advertise live replies — never hidden. */
export function StreamRepliesField({
  value,
  onChange,
  disabled,
  streaming,
}: {
  value: StreamRepliesMode;
  onChange: (value: StreamRepliesMode) => void;
  disabled: boolean;
  streaming: StreamingCapability;
}) {
  const unsupported = streaming.status !== "supported";
  const help =
    streaming.status === "unsupported"
      ? `Unavailable: ${streaming.reason}. Replies appear when they are complete.`
      : streaming.status === "loading"
        ? "Checking whether this gateway supports live replies…"
        : "Show the reply while the model writes it. The complete reply replaces the live text when the model call ends.";
  return (
    <>
      <label className="code-field">
        Stream replies
        <select
          aria-label="Stream replies"
          aria-describedby="code-stream-replies-help"
          value={value}
          disabled={disabled || unsupported}
          onChange={(event) =>
            onChange(event.target.value as StreamRepliesMode)
          }
        >
          <option value="gateway_default">
            {gatewayDefaultStreamLabel(streaming)}
          </option>
          <option value="on">On</option>
          <option value="off">Off</option>
        </select>
      </label>
      <p
        id="code-stream-replies-help"
        className="code-field-help"
        data-streaming={streaming.status}
      >
        {help}
      </p>
    </>
  );
}
