import React from "react";
import { SettingsHelp } from "./settings_help";

/** One place to choose the agent's workflow, tools, knowledge and instructions. */
export function AgentSettings(props: {
  workflow: React.ReactNode;
  tools: React.ReactNode;
  skills: React.ReactNode;
  behavior: React.ReactNode;
  skillCount: number;
  workflowDescription?: string;
}): React.ReactElement {
  return (
    <div className="code-agent-settings">
      <section className="code-agent-section">
        <h3>
          Workflow
          <SettingsHelp label="About workflows">
            {props.workflowDescription || "The workflow defines the agent's steps. Most conversations need no extra setup."}
          </SettingsHelp>
        </h3>
        {props.workflow}
      </section>
      <section className="code-agent-section">
        <h3>Tools</h3>
        {props.tools}
      </section>
      <details className="code-agent-section code-settings-advanced">
        <summary>
          Skills <span className="code-muted">{props.skillCount || "None selected"}</span>
          <SettingsHelp label="About skills">
            Skills add instructions and specialist knowledge to the agent. Only skills offered by the Gateway can be selected.
          </SettingsHelp>
        </summary>
        {props.skills}
      </details>
      <details className="code-agent-section code-settings-advanced">
        <summary>
          Instructions &amp; limits
          <SettingsHelp label="About agent instructions">
            Optional guidance and run limits for this selection. Leave blank to use the workflow defaults.
          </SettingsHelp>
        </summary>
        {props.behavior}
      </details>
    </div>
  );
}
