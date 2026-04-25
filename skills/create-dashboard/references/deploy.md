# Phase 7: Deploy via `cx dashboards create`

Don't tell the user to paste JSON into the Coralogix UI — deploy the dashboard directly.

---

## 1. Pick a folder

List folders and suggest the best match:

```bash
cx dashboards folders list -o json
```

Rank the existing folders by relevance (service name, team, product area) and present the top matches with `AskQuestion`:

- "Folder X (id: `<id>`) — best match by name"
- "Folder Y (id: `<id>`)"
- "Root (no folder)"
- "None of these — I'll create a folder in the Coralogix UI first"

Default to "Root" if nothing fits.

---

## 2. If the user wants a new folder

The `cx` CLI doesn't support folder creation (the Coralogix API doesn't expose folder-create on every tenant). When the user picks "None of these":

1. Ask them to create the folder they want in the Coralogix UI: **Dashboards → Folders → + New folder**.
2. Once they confirm it's created, rerun `cx dashboards folders list -o json` and present the updated list so they can pick the new folder.
3. Proceed to step 3 with the chosen folder id.

---

## 3. Save and deploy

1. Write the verified JSON to `/tmp/cx-dashboard-<slug>.json` (use the file-write tool; don't prescribe a specific shell idiom).
2. Deploy into the chosen folder (omit `--folder` for root):

   ```bash
   cx dashboards create --from-file /tmp/cx-dashboard-<slug>.json --folder <folder-id>
   ```

The CLI generates the `requestId` envelope automatically and prints the created dashboard ID and name on success. Pipe into `-o json` or `-o agents` for structured output.

On failure: show the CLI error verbatim, return to Phase 5 (most common cause: a query that parses locally but the live API rejects), fix, and redeploy.

On success: emit the summary defined in the main `SKILL.md` "Output format for the user" section.

---

## 4. Idempotency note

Each run generates a fresh top-level `id` (21-char nanoid), so re-running this skill creates a *new* dashboard rather than overwriting an existing one. If the user wants to replace an existing dashboard, point them at the Coralogix API's "replace dashboard" endpoint — that's outside this skill's current scope.
