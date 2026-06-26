# Phase 7: Deploy via `cx dashboards create`

Don't tell the user to paste JSON into the Coralogix UI - deploy the dashboard directly.

---

## 1. Pick a folder

List folders and suggest the best match:

```bash
cx dashboards folders list -o json
```

Rank the existing folders by relevance (service name, team, product area) and present the top matches with `AskQuestion`:

- "Folder X (id: `<id>`) - best match by name"
- "Folder Y (id: `<id>`)"
- "Root (no folder)"
- "None of these - I'll create a folder in the Coralogix UI first"

Default to "Root" if nothing fits.

---

## 2. If the user wants a new folder

Ask them for a folder name (and an optional parent folder id - omit for a top-level folder), then create it directly:

```bash
cx dashboards folders create --name "<Folder Name>"
# or, as a sub-folder of an existing one:
cx dashboards folders create --name "<Sub-folder>" --parent-id <parent-folder-id>
```

The command prints the new folder id. Use that id as `--folder` in step 3.

If folder creation fails (most common cause: API key missing the `team-dashboards:Update` permission), fall back to the Coralogix UI - **Dashboards → Folders → + New folder** - then rerun `cx dashboards folders list -o json` and proceed with the chosen id.

---

## 3. Save and deploy

1. Write the verified JSON to `/tmp/cx-dashboard-<slug>.json` (use the file-write tool; don't prescribe a specific shell idiom).
2. Deploy into the chosen folder (omit `--folder` for root):

   ```bash
   cx dashboards create --from-file /tmp/cx-dashboard-<slug>.json --folder <folder-id>
   ```

The CLI generates the `requestId` envelope automatically and prints the created dashboard ID and name on success. Pipe into `-o json` or `-o agents` for structured output.

On failure: show the CLI error verbatim, return to Phase 5 (most common cause: a query that parses locally but the live API rejects), fix, and redeploy.

On success: continue to step 4 below — the workflow is **not finished** until the user has the link.

---

## 4. Share the link (final step — mandatory)

Don't stop at "dashboard created". The very last action is to give the user a clickable link to the dashboard.

Build the URL from the active profile's region and the dashboard `id` returned by `cx dashboards create`:

```
https://<region>.app.coralogix.com/#/dashboards/<id>
```

Region → webapp host mapping:

| Region | Webapp host |
|---|---|
| `us1` / `us2` / `us3` | `us1.app.coralogix.com` / `us2.app.coralogix.com` / `us3.app.coralogix.com` |
| `eu1` / `eu2` | `eu1.app.coralogix.com` / `eu2.app.coralogix.com` |
| `ap1` / `ap2` / `ap3` | `ap1.app.coralogix.com` / `ap2.app.coralogix.com` / `ap3.app.coralogix.com` |
| `stg1` | `stg1.app.coralogix.net` |
| Custom (`https://api.<host>`) | Strip the leading `api.` and use `<host>` (e.g. `api.myenv.coralogix.com` → `myenv.app.coralogix.com`). |

If the custom endpoint doesn't follow the `api.` prefix convention, **omit the link entirely** — do not invent a URL. Use the second ("webapp host cannot be derived") template in `SKILL.md` § "Output format for the user", which drops the markdown link from the `Deployed` line *and* drops the standalone `Open it:` line so the user is never shown a broken URL.

Render the link as a markdown link using the dashboard **name** as the link text, e.g.:

```
Dashboard: **[Order Service - Health](https://eu2.app.coralogix.com/#/dashboards/abc123def456)**
```

Then emit the summary defined in the main `SKILL.md` § "Output format for the user".

---

## 5. Replace an existing dashboard

To update a dashboard that already exists (instead of creating a new one), use the replace workflow:

1. Get the current definition:

   ```bash
   cx dashboards get <dashboard-id> -o json > dashboard.json
   ```

2. Edit `dashboard.json` (change widgets, queries, filters, etc.). The `id` field must remain intact.

3. Deploy the updated version:

   ```bash
   cx dashboards replace --from-file dashboard.json --yes
   ```

This is a full replacement - the entire dashboard definition is overwritten. The `id` field in the JSON determines which dashboard is updated.

Use replace when:
- The user asks to update, modify, or iterate on an existing dashboard.
- You're refining a dashboard after Phase 5 verification found issues.
- The user exported a dashboard and wants to push changes back.

Use create (not replace) when:
- Building a new dashboard from scratch.
- Duplicating an existing dashboard (remove the `id` field first).

---

## 6. Idempotency note

Each `create` run generates a fresh top-level `id` (21-char nanoid), so re-running creates a *new* dashboard rather than overwriting an existing one. Use `replace` to update in place.
