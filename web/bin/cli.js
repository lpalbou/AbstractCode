#!/usr/bin/env node

import { createGatewayUrlResolver, parseAppFlagsOrExit } from "@abstractframework/app-server";

import { createCodeServer } from "./server.js";

// The shared launch flags: --gateway-url (aliases --gateway, --url), --port,
// --host (default 127.0.0.1: the gateway serves this app at /apps/code/),
// --help. Environment variables are legacy aliases only (PORT, HOST,
// ABSTRACTCODE_GATEWAY_URL, ABSTRACTGATEWAY_URL).
const flags = parseAppFlagsOrExit(process.argv.slice(2), {
  appName: "AbstractCode Web",
  command: "abstractcode-web",
  envPrefix: "ABSTRACTCODE",
  defaultPort: 3002,
});

// A flag or environment choice is fixed; otherwise the local gateway pointer
// is followed (re-read when the gateway refuses a connection, so a running
// app follows the gateway onto a new port).
const fixed = flags.gatewayUrlSource === "flag" || flags.gatewayUrlSource.startsWith("env:");
const server = createCodeServer({ defaultGatewayUrl: fixed ? flags.gatewayUrl : createGatewayUrlResolver() });

server.listen(flags.port, flags.host, () => {
  console.log(`AbstractCode Web on http://${flags.host}:${flags.port}/ (gateway ${flags.gatewayUrl}, from ${flags.gatewayUrlSource})`);
});

function shutdown() {
  server.close(() => process.exit(0));
}

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
