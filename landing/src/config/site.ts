export const site = {
  name: "JIG.SH",
  title: "JIG.SH — Agentic Harness for Your Repos",
  description:
    "Jig is a reusable harness that turns a repository into an operating environment for coding agents. A schema-defined contract, MCP runtime, receipts, gates, dev proxy, and a sealed local vault. Built for Rust and Go app repos with React / TypeScript web apps alongside.",
  ogDescription:
    "A reusable harness that turns a repository into an operating environment for coding agents.",
  repoUrl: "https://github.com/bpcakes/jig-sh",
  repoCloneUrl: "https://github.com/bpcakes/jig-sh.git",
  repoHost: "github.com/bpcakes/jig-sh",
  skillsRepoUrl: "https://github.com/bpcakes/jig-skills",
  cratesUrl: "https://crates.io/crates/jig-sh",
  companyUrl: "https://bananapancakes.co",
  branch: "master",
} as const;

export function repoBlob(path: string): string {
  return `${site.repoUrl}/blob/${site.branch}/${path}`;
}

export function repoTree(path: string): string {
  return `${site.repoUrl}/tree/${site.branch}/${path}`;
}
