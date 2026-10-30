# Staged review findings (#109)

`generation_service::review::generate_review(pool, crypto, request, cancelled)`
is the one-shot findings seam. It adds no IPC, coordinator, runner, tools that
execute code, or provider. Callers supply the immutable capture, snapshot-bound
support, and the persisted `ReviewSnapshot` fetched through `review_repo`.
The explicit active configuration and requested model are pinned for one call;
missing/inactive selections fail without a fallback. Credentials stay in memory.

The versioned `review_v1` prompt treats repository text as untrusted data. Tool
arguments and text-only JSON use the same strict schema. Every new finding has
a server UUID, summary, explanation, expected behavior, severity, suggested test,
validated citations, and `suspected` evidence. Empty findings are valid. No
repair, salvage, retry, execution, or reproduction claim is accepted.

Context uses #108's `model_text` and `permits`. Before calling the model, capture
bytes, source identities, modes, and changes must match the persisted manifest.
Line citations must also fit the final supplied excerpts, after optional context
is removed. Metadata-only changes supply old/new modes and paths separately;
their file-level citations omit both line fields. Redaction masks credential
lines and PEM blocks while preserving CR/LF. Ambiguous multiline credential
values exclude the file instead of sending uncertain continuation lines.
Credential value declarations without an explicit same-line semicolon are also omitted:
balanced quotes alone cannot rule out a next-line expression continuation across
languages. Comments and PEM blocks retain their existing line-preserving masking.
HTTP(S) URL userinfo is masked before other checks, preserving hosts, paths, query
strings, and line endings. Complete named authentication function signatures
without credential defaults remain usable context. Ambiguous multiline signatures,
function expressions, and defaults containing credential terms are visibly omitted.

The full serialized provider-neutral request (including escaped content,
messages, model, schema, options, and a framing reserve) must fit 8,000 tokens
and the provider window minus its output reserve. Byte counting conservatively
bounds token estimates. Lowest-ranked optional excerpts are removed first;
required content is never shortened. Output requests at most 6,000 tokens (or
the provider's lower limit), accepts at most 128 KiB / 20 findings, and requires
successful terminal stream completion. Cancellation drops the stream; a
120-second deadline bounds the entire call. Provider errors retain existing
codes and recovery hints, with raw bodies/endpoints removed.

Reviews select `StreamPolicy::SingleAttempt` through the existing provider
trait. The default `stream` path retains artifact generation's tool-to-JSON
compatibility fallback; its warning contains only provider/status, never the
response body. Ollama Cloud retains `ollama-cloud` provenance while using the
factory's authenticated `openai` runtime. The compatible SSE transport maintains
tool IDs across indexed fragments and emits one completion after trailing usage,
preserving cutoff reasons. Both SSE wire formats bound each event to 128 KiB
and HTTP error bodies to 16 KiB before appending bytes; oversized events fail,
while oversized error bodies retain HTTP status classification using a bounded
prefix, including when reading that body is interrupted. Credential declarations
with comments or unmatched delimiters outside quoted values are excluded because
they can conceal multiline continuations.

The returned `ReviewGenerationOutcome` carries findings, exact configuration /
provider / model / prompt provenance, available usage, final `excluded_files`,
and `truncation`. **#110 must persist all of these before emitting completion**:
append exclusions to `ReviewSession.excluded_files` and truncation to the session,
then save findings and usage through `review_repo::update`. The immutable
snapshot's original exclusions remain unchanged. `excludedFiles` and finding
`suggestedTest` are additive optional wire fields, omitted on legacy records;
new model findings always populate `suggestedTest`. Both persist in existing
JSON payloads with matching Rust/Zod validation; no migration is required.

#110 still owns queued/running/terminal transitions, overall 300-second review
deadline, persistence on failure, events, cancellation status, and clean-index /
nothing-reviewable outcomes without a model call. #112 owns execution evidence.
This seam alone does not make the desktop review workflow available.
