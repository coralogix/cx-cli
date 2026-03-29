# Configuration

`cx` stores all configuration under `~/.cx/`.

## Directory Structure

```
~/.cx/
  config.toml              # Global settings
  profiles/
    default.toml           # Default profile
    prod.toml              # Named profile
    staging.toml           # Named profile
```

## Global Config (`~/.cx/config.toml`)

| Key | Default | Description |
|-----|---------|-------------|
| `default_profile` | `"default"` | Profile used when `--profile` is not provided |
| `default_output_format` | `"text"` | Output format when `--output` is not provided (`text`, `json`, `agents`) |
| `max_dataprime_direct_output_size` | `102400` (100 KiB) | Max byte size for non-aggregated DataPrime results in `agents` mode before spilling to a temp file. Set to `-1` to disable |
| `temp_dir` | `"/tmp/"` | Directory for spilled result files |

Example:

```toml
default_profile = "default"
default_output_format = "text"
max_dataprime_direct_output_size = 102400
temp_dir = "/tmp/"
```

## Profile Files (`~/.cx/profiles/<name>.toml`)

Each profile stores credentials and region info for a Coralogix environment.

| Key | Required | Description |
|-----|----------|-------------|
| `api_key` | Yes | Coralogix API key |
| `region` | Yes | Coralogix region (see below) |
| `label` | No | Free-form label (e.g. "production", "staging") |
| `team_id` | No | Coralogix team ID — required for `cx search-fields` |
| `openai_api_key` | No | OpenAI API key for semantic search features |

Example:

```toml
api_key = "cxp_your_api_key_here"
region = "eu1"
label = "production"
team_id = "123456"
openai_api_key = "sk-..."
```

## Regions

| Region | Endpoint |
|--------|----------|
| `us1` | `https://api.us1.coralogix.com` |
| `us2` | `https://api.us2.coralogix.com` |
| `eu1` | `https://api.eu2.coralogix.com` |
| `eu2` | `https://api.eu2.coralogix.com` |
| `ap1` | `https://api.ap1.coralogix.com` |
| `ap2` | `https://api.ap2.coralogix.com` |
| `ap3` | `https://api.ap3.coralogix.com` |
| `stg1` | `https://api.stg1.coralogix.net` |

You can also pass a custom URL as the region value.

## Environment Variables

Environment variables override profile file values:

| Variable | Overrides |
|----------|-----------|
| `CX_PROFILE` | `--profile` flag / `default_profile` |
| `CX_API_KEY` | `api_key` in profile |
| `CX_REGION` | `region` in profile |
| `OPENAI_API_KEY` | `openai_api_key` in profile (takes precedence) |

**Precedence order:** CLI flags > environment variables > profile file > global config defaults.
