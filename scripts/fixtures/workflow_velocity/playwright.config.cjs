// Ownership fields match the generated Rust/React Playwright configuration.
// Tiny HTTP children replace application compilation; no browser is launched.
const { defineConfig } = require(process.env.PROBE_PLAYWRIGHT);
const path = require("node:path");
module.exports = defineConfig({
  testDir: ".",
  testMatch: "ownership.spec.cjs",
  workers: 1,
  retries: 0,
  timeout: 30000,
  reporter: "line",
  outputDir: path.join(process.env.PROBE_CONTROL, process.env.PROBE_LABEL),
  webServer: [process.env.E2E_API_PORT, process.env.E2E_WEB_PORT].map(port => ({
    command: `${JSON.stringify(process.env.PROBE_PYTHON)} server.py ${port}`,
    url: `http://127.0.0.1:${port}/health/ready`,
    timeout: 10000,
    reuseExistingServer: false,
    gracefulShutdown: { signal: "SIGTERM", timeout: 1000 },
  })),
});
