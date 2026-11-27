# Phase 8: Deploy via `cx dashboards create`

Don't tell the user to paste JSON into the Coralogix UI - deploy the dashboard directly.

---

## 1. Pick a folder

List the folders:

```bash
cx dashboards folders list -o json
```

Pick the folder in this order, and say in the final output which one you used:

1. The folder the user named.
2. The best match by service name, team or product area. A folder named after a person is never a match.
3. Root (omit `--folder`).

Ask the user to choose, with `AskQuestion`, only when they said they want to pick the folder. Offer the top matches, "Root (no folder)" and "A new folder" (section 2).

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
   cx dashboards create --from-file /tmp/cx-dashboard-<slug>.json --folder <folder-id> --yes
   ```

The CLI generates the `requestId` envelope automatically and prints the created dashboard ID and name on success. Pipe into `-o json` or `-o toon` for structured output.

On failure: show the CLI error verbatim, return to Phase 5 (most common cause: a query that parses locally but the live API rejects), fix, and redeploy.

On success: continue to step 4 below — the workflow is **not finished** until the user has the link.

---

## 4. Share the link (final step — mandatory)

Don't stop at "dashboard created". The very last action is to give the user a clickable link to the dashboard.

Don't hand-build the URL. `cx dashboards create` / `cx dashboards replace` already print a `View in Coralogix: <url>` line to stderr on success once it can resolve a console link for the active profile - capture that line and reuse the URL verbatim.

If no `View in Coralogix:` line was printed (no console link could be resolved for the profile and no `console_url` override is configured - see `docs/configuration.md` § "Console links"), **omit the link entirely** — do not invent a URL. Use the second (no-link) template in `SKILL.md` § "Output format for the user", which drops the markdown link from the `Deployed` line *and* drops the standalone `Open it:` line so the user is never shown a broken URL.

Render the link as a markdown link using the dashboard **name** as the link text:

```
Dashboard: **[<Name>](<url from the View in Coralogix line>)**
```

Then emit the summary defined in the main `SKILL.md` § "Output format for the user".

---

## 5. Replace an existing dashboard

To update a dashboard that already exists (instead of creating a new one), use the replace workflow:

1. Get the current definition, and keep an untouched copy to compare against in step 3:

   ```bash
   cx dashboards get <dashboard-id> -o json > dashboard.json
   cp dashboard.json dashboard.orig.json
   ```

2. Edit `dashboard.json` (change widgets, queries, filters, etc.). The `id` field must remain intact.

3. Check the edited file, and diff it against the untouched copy:

   ```bash
   cx dashboards check --from-file dashboard.json
   diff <(jq -S . dashboard.orig.json) <(jq -S . dashboard.json)
   ```

   Fix any issue whose `location` falls in a part the diff shows you changed. If an issue points at a part the diff does not show as changed, the fetched JSON came back damaged (for example a table rule property whose `definition` is `{}`): stop and show the user the issues instead of replacing. Do not delete or rewrite those parts to make the check pass, since replacing would make the loss permanent.

4. Deploy the updated version:

   ```bash
   cx dashboards replace --from-file dashboard.json --yes
   ```

This is a full replacement - the entire dashboard definition is overwritten, with no conflict check. The `id` field in the JSON determines which dashboard is updated. Do steps 1 to 4 in one go, and do not reuse a `dashboard.json` fetched earlier in the session. If time has passed since step 1, run `get` again into a fresh file, copy it to a fresh `dashboard.orig.json`, re-apply only the user's edits to that fresh JSON, run step 3 on it again, then run `replace`. Otherwise edits made in the meantime are silently lost.

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
