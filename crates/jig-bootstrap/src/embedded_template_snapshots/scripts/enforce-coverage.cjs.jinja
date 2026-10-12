#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");

const coverageDir = process.env.COVERAGE_DIR ?? "coverage";
const threshold = Number(process.env.COVERAGE_THRESHOLD ?? "0");
const summaryPath = path.join(coverageDir, "coverage-summary.json");

if (!fs.existsSync(summaryPath)) {
  console.log("No coverage summary generated; creating an empty summary.");
  fs.mkdirSync(coverageDir, { recursive: true });
  const empty = {
    total: {
      lines: { pct: 0 },
      functions: { pct: 0 },
      statements: { pct: 0 },
      branches: { pct: 0 },
    },
  };
  fs.writeFileSync(summaryPath, JSON.stringify(empty, null, 2));
}

const summary = JSON.parse(fs.readFileSync(summaryPath, "utf8"));
const total = summary.total ?? {};
const metrics = ["lines", "functions", "statements", "branches"];
const below = [];

for (const metric of metrics) {
  const pct = Number(total[metric]?.pct ?? 0);
  console.log(`${metric}: ${pct}%`);
  if (pct < threshold) {
    below.push(`${metric} (${pct}%)`);
  }
}

if (below.length > 0) {
  console.error(`Coverage below threshold ${threshold}%: ${below.join(", ")}`);
  process.exit(1);
}
