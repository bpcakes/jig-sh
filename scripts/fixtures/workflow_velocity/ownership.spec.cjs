const { test, expect } = require(process.env.PROBE_PLAYWRIGHT);
const fs = require("node:fs");
const path = require("node:path");
test("the invocation owns its two managed HTTP servers", async ({ request }) => {
  for (const port of [process.env.E2E_API_PORT, process.env.E2E_WEB_PORT]) {
    const response = await request.get(`http://127.0.0.1:${port}/health/ready`);
    expect(await response.text()).toBe(`ExampleProject ready ${process.env.PROBE_LABEL}`);
  }
  fs.writeFileSync(path.join(process.env.PROBE_CONTROL, `browser-ready-${process.env.PROBE_LABEL}`), "ready");
  const deadline = Date.now() + 25000;
  while (!fs.existsSync(path.join(process.env.PROBE_CONTROL, "release"))) {
    if (Date.now() > deadline) throw new Error("probe release deadline expired");
    await new Promise(resolve => setTimeout(resolve, 10));
  }
});
