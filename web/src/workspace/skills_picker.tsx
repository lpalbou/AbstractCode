import React, { useEffect, useMemo, useState } from "react";

import { formatError, gatewayRequest } from "./transport";

export type GatewaySkill = {
  name: string;
  description: string;
  trustLevel: string;
  blocked: boolean;
  requiresReview: boolean;
  reasons: string[];
  source?: { source?: string; binding?: string; ambiguous?: boolean };
};

type SkillsInventory = {
  skills: GatewaySkill[];
  warnings: string[];
  /** Where the gateway reads skills from, and how that place was chosen. */
  shelf?: string;
  shelfSource?: string;
};

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

/** Normalize the gateway's `/skills` shelf without inventing trust state. */
export function normalizeSkillsInventory(value: unknown): SkillsInventory {
  const root =
    value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {};
  const rawSkills = Array.isArray(root.skills) ? root.skills : [];
  const skills = rawSkills.flatMap((raw) => {
    const row =
      raw && typeof raw === "object" && !Array.isArray(raw)
        ? (raw as Record<string, unknown>)
        : null;
    const name = text(row?.name);
    if (!row || !name) return [];
    const rawSource =
      row.source && typeof row.source === "object" && !Array.isArray(row.source)
        ? (row.source as Record<string, unknown>)
        : undefined;
    return [
      {
        name,
        description: text(row.description),
        trustLevel: text(row.trust_level) || "unverified",
        blocked: row.blocked === true,
        requiresReview: row.requires_review === true,
        reasons: Array.isArray(row.reasons)
          ? row.reasons.map(text).filter(Boolean)
          : [],
        source: rawSource
          ? {
              source: text(rawSource.source),
              binding: text(rawSource.binding),
              ambiguous: rawSource.ambiguous === true,
            }
          : undefined,
      } satisfies GatewaySkill,
    ];
  });
  const warnings = Array.isArray(root.warnings)
    ? root.warnings.map(text).filter(Boolean)
    : [];
  const shelf = text(root.shelf);
  const shelfSource = text(root.shelf_source);
  return {
    skills: skills.sort((left, right) => left.name.localeCompare(right.name)),
    warnings,
    ...(shelf ? { shelf } : {}),
    ...(shelfSource ? { shelfSource } : {}),
  };
}

const SHELF_SOURCES: Record<string, string> = {
  saved: "set in the gateway's settings",
  env: "set in the gateway's launch environment",
  seeded: "the gateway's built-in shelf",
};

/** An empty skill list, with the gateway's own explanation shown in full. */
export function SkillsEmptyState({
  inventory,
}: {
  inventory: SkillsInventory;
}): React.ReactElement {
  return (
    <div className="code-skills-empty" role="status">
      <p>
        <strong>This gateway offers no skills.</strong>
      </p>
      {inventory.warnings.length ? (
        inventory.warnings.map((warning, index) => (
          <p key={`${warning}:${index}`}>
            {inventory.shelf ? warning : `The gateway has no skill shelf: ${warning}`}
          </p>
        ))
      ) : (
        <p>The gateway gave no reason for the empty list.</p>
      )}
      {inventory.shelf ? (
        <p className="code-field-help">
          Skill shelf: <code>{inventory.shelf}</code>
          {inventory.shelfSource
            ? ` (${SHELF_SOURCES[inventory.shelfSource] || inventory.shelfSource})`
            : ""}
        </p>
      ) : (
        <p className="code-field-help">The gateway did not report a skill shelf location.</p>
      )}
    </div>
  );
}

function trustLabel(skill: GatewaySkill): string {
  const trust = skill.trustLevel.replace(/[_-]+/g, " ");
  const adoption = skill.source?.source
    ? `Adopted from ${skill.source.source}`
    : "Source has no adoption record";
  return `Trust: ${trust}. ${adoption}${skill.requiresReview ? ". Review required" : ""}.`;
}

export function SkillsPicker({
  value,
  onChange,
  disabled,
}: {
  value: string[];
  onChange: (next: string[]) => void;
  disabled: boolean;
}): React.ReactElement {
  const [inventory, setInventory] = useState<SkillsInventory>({
    skills: [],
    warnings: [],
  });
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    const abort = new AbortController();
    setLoading(true);
    setError("");
    void gatewayRequest("/api/gateway/skills", { signal: abort.signal })
      .then((response) => {
        if (!abort.signal.aborted)
          setInventory(normalizeSkillsInventory(response));
      })
      .catch((reason) => {
        if (!abort.signal.aborted) setError(formatError(reason));
      })
      .finally(() => {
        if (!abort.signal.aborted) setLoading(false);
      });
    return () => abort.abort();
  }, [revision]);

  const selected = useMemo(
    () => new Set(value.map(text).filter(Boolean)),
    [value],
  );
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return inventory.skills;
    return inventory.skills.filter((skill) =>
      `${skill.name}\n${skill.description}\n${skill.trustLevel}`
        .toLocaleLowerCase()
        .includes(needle),
    );
  }, [inventory.skills, query]);
  const toggle = (name: string, checked: boolean) => {
    const next = checked
      ? [...selected, name]
      : [...selected].filter((item) => item !== name);
    onChange([...new Set(next)]);
  };

  return (
    <section
      className="code-settings-section"
      aria-labelledby="code-skills-heading"
    >
      <h3 id="code-skills-heading">Skills</h3>
      <p className="code-muted">
        Choose curated guidance for the next turn. The gateway evaluates trust
        and requirements when the run starts; this browser does not load or
        execute skill scripts.
      </p>
      <label className="code-field">
        Find a skill
        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          disabled={disabled || loading}
          placeholder="Search names, descriptions, or trust"
        />
      </label>
      {loading ? (
        <p className="code-muted" role="status">
          Loading skills from the gateway…
        </p>
      ) : null}
      {error ? (
        <div className="code-inline-error" role="alert">
          Skill selection is unavailable: {error}{" "}
          <button
            className="code-text-button"
            type="button"
            onClick={() => setRevision((value) => value + 1)}
            disabled={disabled}
          >
            Retry
          </button>
        </div>
      ) : null}
      {inventory.warnings.length && inventory.skills.length ? (
        <details className="code-disabled-tools">
          <summary>
            Gateway reported {inventory.warnings.length} skills warning
            {inventory.warnings.length === 1 ? "" : "s"}
          </summary>
          {inventory.warnings.map((warning, index) => (
            <p key={`${warning}:${index}`}>{warning}</p>
          ))}
        </details>
      ) : null}
      {!loading && !error && !inventory.skills.length ? (
        <SkillsEmptyState inventory={inventory} />
      ) : null}
      {!loading && !error && inventory.skills.length && !filtered.length ? (
        <p className="code-muted">No skills match your search.</p>
      ) : null}
      {filtered.map((skill) => {
        const blockedReason =
          skill.reasons.join(" ") || "Blocked by gateway policy.";
        const checked = selected.has(skill.name);
        const unavailable = disabled || skill.blocked;
        return (
          <label className="code-field" key={skill.name}>
            <span>
              <input
                type="checkbox"
                checked={checked}
                disabled={unavailable}
                onChange={(event) => toggle(skill.name, event.target.checked)}
              />{" "}
              <strong>{skill.name}</strong>
            </span>
            {skill.description ? (
              <span className="code-field-help">{skill.description}</span>
            ) : null}
            <span className="code-field-help">{trustLabel(skill)}</span>
            {skill.blocked ? (
              <span className="code-error-text">
                Blocked by the gateway: {blockedReason}
              </span>
            ) : null}
          </label>
        );
      })}
    </section>
  );
}
