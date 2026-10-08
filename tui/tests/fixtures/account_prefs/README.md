# Account preferences fixtures (R17-W1)

Recorded from a scratch gateway at abstractgateway origin/main 8da325f (0.13.x), user `alice`, 2026-10-08:

- `get_default.json` — `GET /api/gateway/accounts/me/preferences` (no choice yet: `value: null`).
- `put_coder.json` — `PUT … {"default_workflow": {"abstractcode.agent.v1": "coding-agent:coder"}}` → 200.
- `put_refused_400.json` — `PUT … "nope-bundle:nope"` → 400 `{detail: {reason: "preference_refused", message}}`.

A gateway older than 0.13.1 answers 404 on the route (the terminal then keeps the choice on this computer).
