import React, { useState } from "react";

type PinType =
  | "execution"
  | "string"
  | "number"
  | "boolean"
  | "object"
  | "array"
  | "tools"
  | "provider"
  | "model"
  | "agent"
  | "any";

const PIN_COLORS: Record<PinType, string> = {
  execution: "#FFFFFF",
  string: "#FF00FF",
  number: "#00FF00",
  boolean: "#FF0000",
  object: "#00FFFF",
  array: "#FF8800",
  tools: "#FF8800",
  provider: "#00D2FF",
  model: "#9D4EDD",
  agent: "#4488FF",
  any: "#888888",
};

type PinInfo = {
  type: PinType;
  label: string;
  shape: string;
  description: string;
};

const PIN_INFO: PinInfo[] = [
  { type: "execution", label: "Execution", shape: "▷", description: "Controls flow order" },
  { type: "string", label: "String", shape: "●", description: "Text data" },
  { type: "number", label: "Number", shape: "●", description: "Integer or float" },
  { type: "boolean", label: "Boolean", shape: "◇", description: "True/False" },
  { type: "object", label: "Object", shape: "●", description: "JSON objects" },
  { type: "array", label: "Array", shape: "■", description: "Collections" },
  { type: "tools", label: "Tools", shape: "■", description: "Tool allowlist (string[])" },
  { type: "provider", label: "Provider", shape: "●", description: "LLM provider id/name (string-like)" },
  { type: "model", label: "Model", shape: "●", description: "LLM model id/name (string-like)" },
  { type: "agent", label: "Agent", shape: "⬢", description: "Agent reference" },
  { type: "any", label: "Any", shape: "●", description: "Accepts any type" },
];

export function PinLegend(): React.ReactElement {
  const [collapsed, set_collapsed] = useState(true);

  return (
    <div className="pin-legend">
      <button className="pin-legend-toggle mono" type="button" onClick={() => set_collapsed((v) => !v)} title="Pin Type Legend">
        {collapsed ? "?" : "×"} Pin Types
      </button>

      {!collapsed ? (
        <div className="pin-legend-content">
          {PIN_INFO.map((info) => (
            <div key={info.type} className="pin-legend-row">
              <span className="pin-legend-shape" style={{ color: PIN_COLORS[info.type] }}>
                {info.shape}
              </span>
              <span className="pin-legend-label mono">{info.label}</span>
              <span className="pin-legend-desc mono">{info.description}</span>
            </div>
          ))}

          <div className="pin-legend-rules mono">
            <strong>Connection Rules:</strong>
            <ul>
              <li>Execution connects to Execution only</li>
              <li>"Any" type accepts all data types</li>
              <li>Same types always compatible</li>
              <li>Array and Object are interchangeable</li>
              <li>"Tools" is compatible with Array (specialized string[])</li>
            </ul>
          </div>
        </div>
      ) : null}
    </div>
  );
}

