import { describe, expect, it } from "vitest";

import { normalizeSkillsInventory } from "./skills_picker";

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
