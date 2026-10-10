# Account preferences fixtures (R17-W1)

Recorded from a scratch gateway at abstractgateway origin/main 8da325f (0.13.x), user `alice`, 2026-10-08:

- `get_default.json` — `GET /api/gateway/accounts/me/preferences` (no choice yet: `value: null`).
- `put_coder.json` — `PUT … {"default_workflow": {"abstractcode.agent.v1": "coding-agent:coder"}}` → 200.
- `put_refused_400.json` — `PUT … "nope-bundle:nope"` → 400 `{detail: {reason: "preference_refused", message}}`.

A gateway older than 0.13.1 answers 404 on the route (the terminal then keeps the choice on this computer).

Round 18 (2026-10-10): `get_default.json` and `put_coder.json` gained the gateway's `spoken_language`
block and `preferences.spoken_language` (`"auto"`), generated with
`abstractgateway.spoken_language.preferences_block()` from the round-18 gateway worktree
(`round18/2026-10-10` d70aee3, AbstractVoice's language list); new:

- `put_spoken_fr.json` — `PUT … {"spoken_language": "fr"}` → 200 (the block's `value` is `"fr"`).
- `put_spoken_refused_400.json` — `PUT … {"spoken_language": "xx"}` → 400 `{detail: {reason: "preference_refused", key: "spoken_language", message}}` (AbstractVoice's `refusal_sentence("xx")`).
