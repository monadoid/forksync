import { escapeHtml, renderBootstrapCommand } from "./command.js";

function renderSourceCard(source) {
  return `
    <label class="card">
      <input type="checkbox" name="source" value="${escapeHtml(source.id)}" data-source-card />
      <div>
        <div class="card-title">${escapeHtml(source.display_name)}</div>
        <div class="card-meta">${escapeHtml(source.source_repo)} # ${escapeHtml(source.tracked_branch)}</div>
        <div class="card-summary">${escapeHtml(source.summary)}</div>
      </div>
    </label>
  `;
}

export function renderHtmlPage(sources, query = "") {
  const cards = sources.map(renderSourceCard).join("\n");
  const previewCommand = renderBootstrapCommand([]);

  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>ForkSync Registry</title>
    <style>
      :root {
        color-scheme: light;
        --bg: #f4f1ea;
        --panel: #fffdf8;
        --text: #171717;
        --muted: #6b6259;
        --accent: #0f766e;
        --accent-2: #115e59;
        --border: #ded7cb;
      }
      body {
        margin: 0;
        font-family: ui-sans-serif, system-ui, sans-serif;
        background: linear-gradient(180deg, #f7f2e9 0%, var(--bg) 100%);
        color: var(--text);
      }
      .wrap {
        max-width: 1080px;
        margin: 0 auto;
        padding: 40px 20px 64px;
      }
      .hero {
        display: grid;
        gap: 12px;
        margin-bottom: 24px;
      }
      h1 { margin: 0; font-size: clamp(2rem, 4vw, 3.5rem); letter-spacing: -0.05em; }
      p { margin: 0; line-height: 1.5; color: var(--muted); }
      .toolbar {
        display: grid;
        gap: 12px;
        grid-template-columns: minmax(0, 1fr) auto;
        align-items: end;
        margin: 24px 0;
      }
      input[type="search"], button, pre {
        font: inherit;
      }
      input[type="search"] {
        width: 100%;
        padding: 14px 16px;
        border-radius: 14px;
        border: 1px solid var(--border);
        background: var(--panel);
      }
      button {
        padding: 14px 18px;
        border: 0;
        border-radius: 14px;
        background: var(--accent);
        color: white;
        cursor: pointer;
      }
      button:hover { background: var(--accent-2); }
      .grid {
        display: grid;
        gap: 14px;
        grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
      }
      .card {
        display: grid;
        grid-template-columns: auto 1fr;
        gap: 14px;
        align-items: start;
        padding: 16px;
        border: 1px solid var(--border);
        border-radius: 18px;
        background: rgba(255,255,255,0.82);
        box-shadow: 0 8px 32px rgba(0,0,0,0.04);
      }
      .card-title { font-weight: 700; }
      .card-meta, .card-summary { color: var(--muted); font-size: 0.95rem; }
      .card-summary { margin-top: 8px; }
      .command-box {
        margin-top: 24px;
        padding: 16px;
        border-radius: 18px;
        border: 1px solid var(--border);
        background: var(--panel);
      }
      pre {
        margin: 0;
        overflow-x: auto;
        white-space: pre-wrap;
        word-break: break-word;
      }
      .muted { font-size: 0.92rem; color: var(--muted); }
    </style>
  </head>
  <body>
    <main class="wrap">
      <section class="hero">
        <h1>ForkSync Registry</h1>
        <p>Browse public fork sources, pick the ones you want, and copy a generated bootstrap command.</p>
      </section>

      <section class="toolbar">
        <label>
          <span class="muted">Search public sources</span>
          <input id="search" type="search" placeholder="owner/repo, upstream, summary" value="${escapeHtml(query)}" />
        </label>
        <button id="refresh">Refresh</button>
      </section>

      <section id="sources" class="grid">${cards || `<p class="muted">No matching public sources yet.</p>`}</section>

      <section class="command-box">
        <div class="muted">Generated bootstrap command</div>
        <pre id="command">${escapeHtml(previewCommand)}</pre>
      </section>
    </main>

    <script type="module">
      const search = document.getElementById("search");
      const refresh = document.getElementById("refresh");
      const sources = document.getElementById("sources");
      const command = document.getElementById("command");

      async function loadSources() {
        const query = search.value.trim();
        const response = await fetch(\`/api/sources?query=\${encodeURIComponent(query)}\`);
        const payload = await response.json();
        sources.innerHTML = payload.sources.length
          ? payload.sources.map((source) => \`
              <label class="card">
                <input type="checkbox" name="source" value="\${source.id}" data-source-card />
                <div>
                  <div class="card-title">\${source.display_name}</div>
                  <div class="card-meta">\${source.source_repo} # \${source.tracked_branch}</div>
                  <div class="card-summary">\${source.summary || ""}</div>
                </div>
              </label>
            \`).join("")
          : '<p class="muted">No matching public sources yet.</p>';
        bindSelection();
        updateCommand();
      }

      async function updateCommand() {
        const selected = Array.from(document.querySelectorAll('[data-source-card]:checked')).map((item) => item.value);
        const response = await fetch('/api/bootstrap-command', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ source_ids: selected })
        });
        const payload = await response.json();
        command.textContent = payload.command;
      }

      function bindSelection() {
        Array.from(document.querySelectorAll('[data-source-card]')).forEach((input) => {
          input.addEventListener('change', updateCommand);
        });
      }

      refresh.addEventListener('click', loadSources);
      search.addEventListener('input', () => window.clearTimeout(window.__forksyncTimer));
      search.addEventListener('change', loadSources);
      bindSelection();
      updateCommand();
    </script>
  </body>
</html>`;
}
