# Using the Coralogix CLI (`cx`) for the key, region and verification

Use this when `cx` is installed and `cx profiles list` shows at least one profile. If `cx` is installed but has no profile, the user can sign in with `cx init --oauth --region <region>` (existing account) or `cx init --signup --region <region>` (new account); both open the browser and wait for the user, so run them in the background and pass the printed URL on.

## 1. Pick the profile — before any key operation

The key and the SDK region must come from **one profile**. The active profile is `-p <name>` if the user named one, else `$CX_PROFILE` if set, else the `DEFAULT` row of `cx profiles list`. With several profiles and none named, **ask which Coralogix team the app belongs to**, then pass `-p <name>` to every `cx iam` and `cx dataprime` call.

`coralogixDomain` is that profile's `REGION` in upper case. `cx profiles` and `cx docs` are local commands and take no `-p`.

## 2. Reuse an existing RUM key

```bash
cx iam api-keys send-data-keys -p <profile> -o json
```

This lists ingest-only keys with their `presets`. Never pick a key by its name.

- A row with `presets: ["SendRUMData"]`, `active: "yes"` and `hashed: false` is reusable; its `value` is the public key.
- A row with empty `presets` may be a custom RUM key: `cx iam api-keys get <id> -p <profile> -o json`, and reuse it only if `keyInfo.active` is `true` and `keyInfo.keyPermissions` has `SendRUMData` as its only preset (or `rum-ingress:SendData` as its only permission). The value is `keyInfo.value`; a hashed key can't be reused.
- Never use a `SendData` key: it is secret and carries no RUM permission.

Write the value straight into the git-ignored env file; don't echo it into the chat.

## 3. Or create one (needs the user's yes)

Describe the key to the user and get explicit approval before running this with `--yes`:

```bash
cat > /tmp/rum-key.json <<'EOF'
{
  "name": "<app-name> RUM",
  "owner": { "teamId": <team-id> },
  "keyPermissions": { "presets": ["SendRUMData"], "permissions": [] },
  "hashed": false
}
EOF
cx iam api-keys create --from-file /tmp/rum-key.json -p <profile> --yes -o json
```

- `teamId`: `cached_team_id` in `~/.cx/profiles/<profile>.toml`, or the `owner.teamId` of any key in `cx iam api-keys list -p <profile> -o json`, or ask.
- The response's `value` is the public key. `SendRUMData` is standalone: never add other presets or permissions.
- **Permission denied:** RUM keys are data-ingest keys. `send-data-keys` warning `could not fetch legacy send-data keys` means the role lacks `DATA-INGEST-API-KEYS:READCONFIG`; a failure on `get` or `create` means it lacks `DATA-INGEST-API-KEYS:MANAGE`. Don't retry with other payloads; ask the user to create the key in **Settings → API Keys** (type **Send Your Data**, preset **`SendRUMData`**) or have an admin do it.

## 4. Source-map key

The upload key (team key, **`SourceMapping`** preset) is secret and belongs in CI secrets. Don't create it from here; tell the user to create it in **Settings → API Keys** and store it as `CORALOGIX_SOURCEMAP_API_KEY` in their CI. After the first upload, `cx rum source-maps list --application <app> -p <profile>` shows the releases that have maps.

## 5. Verify data

```bash
cx dataprime query --source rum.events \
  "filter \$d.version_metadata.app_name == '<application>' | limit 10" -p <profile> -o json
```

An empty result right after setup is normal: the SDK sends in batches once someone loads the app. Retry a minute after the page has been opened.
