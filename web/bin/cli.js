#!/usr/bin/env node

import { createCodeServer } from "./server.js";

const port = process.env.PORT || 3002;
const host = process.env.HOST || "0.0.0.0";
const server = createCodeServer();

server.listen(port, host, () => {
  console.log(`
╔════════════════════════════════════════════════════╗
║      AbstractCode Web UI is running!               ║
╚════════════════════════════════════════════════════╝

  🌐 Local:   http://localhost:${port}
  🌐 Network: http://${host}:${port}

  💻 Browser-based coding assistant
  🔗 Connect to AbstractGateway in settings
  🚀 Start coding with durable agent sessions

  Press Ctrl+C to stop
`);
});

function shutdown() {
  console.log("\n\n👋 Shutting down AbstractCode Web...\n");
  server.close(() => process.exit(0));
}

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
