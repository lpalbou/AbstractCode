import { describe, expect, it } from "vitest";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { SkillsEmptyState, normalizeSkillsInventory } from "./skills_picker";

describe("normalizeSkillsInventory", () => {
  it("preserves gateway trust, adoption, and block verdicts without inventing availability", () => {
    const inventory = normalizeSkillsInventory({
      skills: [
        {
          name: "blocked",
          description: "No",
          trust_level: "blocked",
          blocked: true,
          requires_review: true,
          reasons: ["advisory match"],
          source: { source: "curated", binding: "tree_hash" },
        },
        {
          name: "ready",
          description: "Yes",
          trust_level: "validated",
          blocked: false,
          requires_review: false,
        },
      ],
      warnings: ["#FALLBACK shelf partial"],
    });
    expect(inventory.skills.map((skill) => skill.name)).toEqual([
      "blocked",
      "ready",
    ]);
    expect(inventory.skills[0]).toMatchObject({
      blocked: true,
      requiresReview: true,
      reasons: ["advisory match"],
      source: { source: "curated", binding: "tree_hash" },
    });
    expect(inventory.warnings).toEqual(["#FALLBACK shelf partial"]);
  });

  it("rejects malformed rows instead of displaying an invented selectable skill", () => {
    expect(
      normalizeSkillsInventory({
        skills: [{ description: "missing name" }, "not a row"],
      }).skills,
    ).toEqual([]);
  });
});

describe("empty skills list", () => {
  it("reads the shelf and its source", () => {
    expect(
      normalizeSkillsInventory({ skills: [], shelf: "/data/skills/registry", shelf_source: "seeded", warnings: [] }),
    ).toMatchObject({ shelf: "/data/skills/registry", shelfSource: "seeded" });
  });

  it("shows the gateway's warnings expanded, not collapsed", () => {
    const html = renderToStaticMarkup(
      React.createElement(SkillsEmptyState, {
        inventory: normalizeSkillsInventory({ skills: [], warnings: ["the shelf path /x does not exist"] }),
      }),
    );
    expect(html).not.toContain("<details");
    expect(html).toContain("This gateway offers no skills.");
    expect(html).toContain("The gateway has no skill shelf: the shelf path /x does not exist");
    expect(html).toContain("did not report a skill shelf location");
  });

  it("names the shelf and its source in the gateway's own word", () => {
    const html = renderToStaticMarkup(
      React.createElement(SkillsEmptyState, {
        inventory: normalizeSkillsInventory({
          skills: [],
          shelf: "/data/skills/registry",
          shelf_source: "seeded",
          warnings: ["the shelf holds no skill folders"],
        }),
      }),
    );
    expect(html).toContain("the shelf holds no skill folders");
    expect(html).toContain("/data/skills/registry");
    expect(html).toContain("(source: seeded)");
  });

  it("says so when the gateway gives no reason", () => {
    const html = renderToStaticMarkup(
      React.createElement(SkillsEmptyState, { inventory: normalizeSkillsInventory({ skills: [] }) }),
    );
    expect(html).toContain("gave no reason");
  });
});
