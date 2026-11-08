import React from "react";
import {
  IterationLimitField,
  ProviderModelPicker,
  ToolModeEditor,
  type IterationLimitApplied,
  type IterationLimitDefault,
  TOOL_LAYER_STORE_WHERE,
  type ToolCatalogItem,
  type ToolModeLayer,
  type SpeculationValue,
  type WorkspaceRequest,
} from "@abstractframework/ui-kit";
import { CodeWorkspaceFolders, type RunWorkspace } from "./workspace_folders";
import { SkillsPicker } from "./skills_picker";
import { SettingsHelp } from "./settings_help";
import { modelDiscovery } from "./model_discovery";
import type { LegacyToolPreferences } from "./tool_layers";
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
  /**
   * The person's tool choices (Off / Ask / Auto per tool, sparse; kit ToolModeLayer). A
   * conversation's lives in the kit's tool layer store (this app, this browser), an automation's in
   * its definition (`_runtime.allowed_tools` / `tool_policy`). Empty = the gateway's defaults.
   */
  toolLayer: ToolModeLayer;
  /** A pre-0033 Permissions tier / tool picker kept until its one-time migration (tool_layers.ts). */
  legacyTools?: LegacyToolPreferences;
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
  toolLayer: {},
  skills: [],
  workflow: "@default",
  streamReplies: "gateway_default",
};
/** The settings panels SettingsContent renders (Workflow and Voice have their own components). */
export type SettingsTab = "model" | "workspace" | "tools" | "skills" | "behavior";

/** One rail panel's preference sections (Model, Workspace, Tools or Skills). No drawer or navigation wrappers. */
export type SettingsContentProps = {
  tab: SettingsTab;
  value: RunPreferences;
  onChange: (value: RunPreferences) => void;
  /** The gateway's tools as the kit reads them (GET discovery/tools). */
  toolCatalog: readonly ToolCatalogItem[];
  /** The catalog's warnings in the gateway's words. */
  toolWarnings?: readonly string[];
  /** Tools tab: a one-time notice above the editor (the Permissions migration), with its dismiss. */
  toolsNotice?: { text: string; onDismiss: () => void } | null;
  /** Tools tab: an automation (its definition) instead of this app's defaults. */
  automationTools?: boolean;
  disabled: boolean;
  lockedReasonId?: string;
  defaultModel?: { provider: string; model: string };
  workflowDefault?: boolean;
  streaming?: StreamingCapability;
  /** The gateway's iteration default (`capabilities.agent_limits.max_iterations`, kit iterationLimitDefaultFrom); null = not served. */
  iterationDefault?: IterationLimitDefault | null;
  /** The last start's `limits` receipt ({max_iterations, source}) for "Last run: …". */
  lastRunLimits?: IterationLimitApplied | null;
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
  tab, value, onChange, toolCatalog, toolWarnings, toolsNotice, automationTools, disabled, defaultModel,
  workflowDefault, streaming = STREAMING_LOADING, iterationDefault, lastRunLimits,
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
                layout="compact"
                optionsDisclosure
                defaultModeLabel={
                  workflowDefault ? "Workflow default" : "Gateway default"
                }
                value={value}
                onChange={update}
                disabled={disabled}
                effectiveDefault={defaultModel}
                enableSpeculation
                inheritLabel="Gateway default"
                fetchProviders={modelDiscovery.fetchProviders}
                fetchModels={modelDiscovery.fetchModels}
                fetchModelCapabilities={modelDiscovery.fetchModelCapabilities}
                // The kit names the gateway default ("Gateway default · provider / model ·
                // reasoning · MTP depth"); a workflow's own defaults are said here instead.
                {...(workflowDefault
                  ? { defaultHint: defaultModel ? `Workflow default: ${defaultModel.provider} · ${defaultModel.model}.` : undefined }
                  : { fetchGatewayDefault: modelDiscovery.fetchGatewayDefault })}
              />
            </section>
            <details className="code-settings-advanced">
              <summary>{hideStreamReplies ? "Limits" : "Limits & streaming"} <SettingsHelp label="About model limits">Leave these unchanged to use the workflow and Gateway defaults. Context limits control how much conversation the model can use; the iteration limit, how many steps the agent may take before it stops.</SettingsHelp></summary>
              <section className="code-settings-section">
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
                {hideStreamReplies ? null : (
                  <StreamRepliesField
                    value={value.streamReplies}
                    onChange={(streamReplies) => update({ streamReplies })}
                    disabled={disabled}
                    streaming={streaming}
                  />
                )}
                <IterationLimitField
                  className="code-field"
                  value={value.maxIterations}
                  onChange={(maxIterations) => update({ maxIterations })}
                  disabled={disabled}
                  gatewayDefault={iterationDefault}
                  applied={lastRunLimits}
                />
              </section>
            </details>
          </>
        ) : tab === "behavior" ? (
          <section className="code-settings-section">
            <label className="code-field">
              Additional instructions
              <textarea
                rows={3}
                value={value.system}
                placeholder="Project conventions or guidance for this conversation…"
                disabled={disabled}
                onChange={(e) => update({ system: e.target.value })}
              />
            </label>
          </section>
        ) : tab === "workspace" ? (
          <section className="code-settings-section">
            {workspaceRootFixed ? (
              <p className="code-field-help" data-setting="workspace-root-fixed">
                Run folder <strong>This automation</strong> <SettingsHelp label="About the automation folder">Each run uses this automation’s private folder. Folder access below controls which additional folders it can use.</SettingsHelp>
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
            {toolsNotice ? (
              <p className="code-notice" role="status">
                {toolsNotice.text}
                <button type="button" aria-label="Dismiss notice" onClick={toolsNotice.onDismiss}>
                  Dismiss
                </button>
              </p>
            ) : null}
            <ToolModeEditor
              compact
              catalog={toolCatalog}
              layer="app"
              value={value.toolLayer}
              onChange={(toolLayer) => update({ toolLayer })}
              onReset={() => update({ toolLayer: {} })}
              disabled={disabled}
              warnings={toolWarnings}
              savedWhere={automationTools ? "Saved in this automation's definition" : TOOL_LAYER_STORE_WHERE}
              note={
                automationTools
                  ? "Auto tools run without asking at every occurrence; Ask tools wait for you; Off tools are not offered. Gateway restrictions always apply."
                  : "Auto tools run without asking, also when this app is closed; Ask tools wait for you; Off tools are not offered. Gateway restrictions always apply."
              }
            />
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
      <span id="code-stream-replies-help" data-streaming={streaming.status}><SettingsHelp label="About streaming">{help}</SettingsHelp></span>
    </>
  );
}
