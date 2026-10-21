# NEXUS Canvas Registry

Each n8n sub-canvas is an isolated domain worker with a deterministic input/output contract.
The master supervisor routes to these based on intent classification.

## Registry

| canvas_id | display_name | webhook | required_credentials | description |
|---|---|---|---|---|
| `email.summarize` | Email Summary | Execute Workflow | google | Summarize recent Gmail messages |
| `github.pr_check` | GitHub PR Check | Execute Workflow | github | Check open PRs for a repository |
| `calendar.peek` | Calendar Peek | Execute Workflow | google | Show upcoming Google Calendar events |
| `general.chat` | General Chat | Execute Workflow | (none) | Free-form conversation via Ollama/Hermes |

## Input Contract (all canvases)

```json
{
  "transcript": "string — the user's speech",
  "sessionId": "string — for correlation",
  "userId": "string — for credential lookup",
  "deviceId": "string — for auth",
  "credentials": {
    "google": { "access_token": "ya29...", "scopes": "..." },
    "github": { "access_token": "gho_..." },
    "api_keys": { "claude": "sk-...", "devin": "..." }
  },
  "args": { "repo": "Engine-NEXUS/NEXUS-Agent", "since": "24h", "days": 1 }
}
```

## Output Contract (all canvases)

```json
{
  "summary": "string — human-readable result (spoken by ElevenLabs)",
  "email_count": 10,
  "pr_count": 3,
  "event_count": 5
}
```

The `summary` (or `reply_text` for general.chat) field is what the sidecar sends to ElevenLabs for TTS.

## Adding a New Canvas

1. Create a new n8n workflow with an Execute Workflow Trigger.
2. Define the input schema (what args it expects).
3. Define the output schema (what it returns).
4. Add the canvas to this registry table.
5. Add the intent to the master supervisor's Classify Intent node (the `enum` in the JSON schema).
6. Add a route in the master supervisor's Switch node.
7. Add an Execute Workflow Trigger node pointing to the new workflow ID.

## Credential Injection

The sidecar injects the user's OAuth tokens and API keys into the webhook payload.
Canvases extract credentials from the input and use them in HTTP Request nodes.

**Never store credentials in n8n credential manager for multi-user scenarios.**
The sidecar owns the credential vault and injects per-user tokens at runtime.

## Error Handling

If a canvas requires credentials that aren't connected, it should throw an error:
```
"GitHub not connected — ask user to connect GitHub in setup"
```

The master supervisor's Aggregate node will pass this through as the reply_text,
and the sidecar will speak it to the user.
