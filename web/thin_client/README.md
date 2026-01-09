# AbstractCode Thin Client (Web/PWA)

This is a **gateway-only** thin client UI for AbstractFramework:
- connect to a Run Gateway (`/api/gateway/*`)
- render by replaying/streaming the **ledger**
- act by submitting **durable commands**

See:
- Backlog: `docs/backlog/completed/317-abstractcode-react-thin-client-web-pwa-ios-dev-deploy.md`
- iPhone guide: `docs/guide/deployment-iphone.md`

## Local dev
```bash
cd abstractcode/web/thin_client
npm install
npm run dev
```

## Start a workflow
- Run a gateway (`abstractgateway`) with `ABSTRACTGATEWAY_WORKFLOW_SOURCE=bundle` and `ABSTRACTGATEWAY_FLOWS_DIR` pointing to a directory containing one or more `*.flow` bundles.
- For LLM/tool/agent workflows in bundle mode, configure:
  - `ABSTRACTGATEWAY_PROVIDER` and `ABSTRACTGATEWAY_MODEL`
  - `ABSTRACTGATEWAY_TOOL_MODE=passthrough` (default) or `ABSTRACTGATEWAY_TOOL_MODE=local` (dev only)
- In the UI:
  - set `bundle_id` (optional if only one bundle is loaded),
  - set `flow_id`,
  - set `input_data` JSON (keys match the VisualFlow `On Flow Start` output pins),
  - click “Start run”.
