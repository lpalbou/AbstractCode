# 0018 — Shared visual and voice identity for agents and entities

- **Status:** proposed
- **Created:** 2026-10-30
- **Area:** Gateway, entity runtime, thin clients, UI libraries
- **Work item:** abstractcode-0018
- **ADR impact:** required before implementing a cross-client authority/migration contract.

## Summary and current code reality

The operator wants agents, especially named persistent entities, recognizable by
appearance and voice across clients. Entity voice already lives in the entity
home's voice.yaml with full provider/model/voice identity and anti-mixing rules
(`abstractgateway/entity_voice.py`). Appearance and voice mutation already have
admin/creator authorization (`entity_settings_access.py`); native clients and
watch orb styles differ. Code companion preferences are initially client-local.
These presentation identities are not authentication credentials.

## Scope for later planning

Audit existing entity appearance routes, account preferences and all client
selectors before defining a shared read/resolve contract. Preserve entity-home
ownership and full voice identity; do not create a competing settings database.
Define named-agent defaults versus entity-owned values versus explicit device
presentation/accessibility overrides. Keep microphone/output device, volume,
reduced motion and image-capability selection local. Shared identity describes
style/colour/asset and synthesis voice, not a renderer implementation or tokens.
Resolve human labels from catalogs, use stable style IDs, and define invalid or
unavailable voice/asset behavior without mixing identities. Reuse audited
settings authorization and event invalidation so thin clients converge.

Plan migration of existing watch style letters, iPhone/desktop voice preferences
and Code companion settings. Avoid overwriting entity identity with a transient
client override. Evaluate export/import portability and any asset ownership,
size/format/security constraints. Explicitly decide whether side companions
represent the main agent or a separate named identity before shared persistence.

## Validation and non-goals

Future acceptance: same entity renders recognizably and uses the same resolved
voice on Code, systray, iPhone and watch; updates propagate and remain authorized;
offline labels/capability degradation are honest; two providers never share an
unqualified voice ID. Add migration and contract tests before writes. No backend
redesign, new service, model pool, or identity migration is authorized by this
proposal. Parent implementation0016 remains local and opt-in.
