function normalizeSourceRef(source) {
  const repo = source.source_repo ?? source.sourceRepo;
  const branch = source.tracked_branch ?? source.trackedBranch;
  if (!repo || !branch) {
    throw new Error("source record missing repo or branch");
  }

  return `${repo}#${branch}`;
}

export function renderBootstrapCommand(sources) {
  if (!sources.length) {
    return "pnpx forksync init";
  }

  const args = sources
    .map((source) => `--source '${normalizeSourceRef(source)}'`)
    .join(" ");

  return `pnpx forksync init ${args}`;
}

export function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
