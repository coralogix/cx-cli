# Pitfalls

Symptom-driven debugging for a GenAI integration that is wired but not rendering correctly.

| Symptom | Cause | Fix |
|---|---|---|
| Spans visible in the Spans Explorer, nothing in AI Center | Traces stay in the Frequent Search tier | Route traces to the S3 archive; AI Center reads only the archive tier |
| The application never appears in the catalog | Spans lack `gen_ai.provider.name` or `gen_ai.input.messages`, so they are not detected as GenAI | Set both; check the provider value is a semconv well-known value, not a vendor string |
| No spans at all | The instrumentor ran before the provider library was imported and had nothing to patch | Call the instrumentor after importing the provider library, or in the library's documented order |
| No spans, or the exporter points at the wrong endpoint | OTel was initialized before the environment file was loaded | Load environment variables before initializing the SDK and the instrumentor |
| The last spans of every run are missing | A batch span processor buffered them and the process exited | Call `force_flush()` / `shutdown()` on exit; in services wire provider shutdown into teardown, ordered last |
| Random spans missing, tokens and cost under-counted | Head or tail sampling drops GenAI spans | Exclude GenAI spans from the sampling policy, or sample whole traces only |
| Prompts and responses empty in the UI | Content capture is off, or the installed parser rejected the value and degraded to no content with only a log warning | Set the library's own switch (`OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` or `TRACELOOP_TRACE_CONTENT`) to a value that version's parser accepts, then confirm on a real span |
| Message JSON truncated mid-payload and unparseable | An attribute value length limit is set | Unset `OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT` and `OTEL_SPAN_ATTRIBUTE_VALUE_LENGTH_LIMIT` |
| Large prompts drop while small ones arrive | The OTLP gRPC receiver's default 4 MiB message limit rejects the batch | Raise `max_recv_msg_size_mib` on the receiver and lower the batch processor's `send_batch_max_size` |
| Messages present but unreadable in the conversation view | A language-native `repr` / `toString` blob was stuffed into a content field instead of structured parts | Serialize string-encoded JSON with typed parts (`text`, `tool_call`, `tool_call_response`) |
| A message renders with no author | The message object is missing `role` | Include `role` on every entry of `gen_ai.input.messages` and `gen_ai.output.messages` |
| Message attributes arrive as an object, not text | The attribute was set to a native object rather than string-encoded JSON | Serialize to a JSON string before setting the attribute |
| Every application lands under one name | The collector exporter uses only its static application and subsystem names | Set `application_name_attributes` / `subsystem_name_attributes` on the exporter and emit `cx.application.name` / `cx.subsystem.name` |
| One provider on every call while the model varies | An instrumentor behind a gateway or proxy hard-codes a static provider | Override `gen_ai.provider.name` per call from the model actually routed |
| Tokens and cost roughly doubled | Two instrumentors cover the same call path | Scope emitters so each call path has exactly one, per route, key, or service |
| Cost shows 0 with tokens present | The model is unknown to Coralogix pricing (self-hosted or newly released) | Register it with `cx ai-center model-pricing set` (JSON file, `--yes`) |
| No agent graph despite agent spans | Agent executions are typed as tool or custom spans | Type each agent execution as `invoke_agent` with `gen_ai.agent.name`; the graph renders only from those |
| The agent and tool hierarchy disappears after disabling vendor telemetry | The SDK's "disable tracing" switch also deletes span generation, not just the vendor upload | Clear the vendor's exporter or API key instead, and keep a test fixture that clears the switch |
| Errors show 0 while calls are failing | Failures are caught and logged without touching span status | Set span status `ERROR` and record the exception; the UI counts only `otel.status_code == ERROR` |
| Model or token fields unrecognized | Non-registry attribute names (`gen_ai.model`, invented vendor keys) | Use the exact semconv names from the registry and the Coralogix inventory |
| Flat traces, no step visible | Every span is emitted at the root with no parent context | Nest spans: GenAI spans under the request-handler root, tool backend calls inside tool spans |
| The verified trace does not match the code under test | A stale process, another checkout, or a dev shortcut produced it | Filter on the probe marker and re-run through the real entry point before auditing |
| Prompts contain personal or confidential data | Content capture exports the raw payload | Mask or exclude sensitive fields before the span is created, and keep capture off where it is not allowed |
| Messages captured, conversation view empty | Content on `invoke_agent`/wrapper span; UI parses those as metadata-only | Put messages on the `chat` span of each model call |
| One provider for every call behind a gateway, models vary | Provider set from the API shape or a constant | Derive per call from the routed/response model |
| One span per agent turn, no latency or tool detail | Turn-level summary span | One `chat` span per model response, `execute_tool` children |
| Traces flow locally, nothing after restart/deploy | Switches passed inline on a command line | Wire ON in the environment's config file, cite file:line |
