-- Embedding provider selection (plan/EMBEDDING_PROVIDER_SELECT.md).
--
-- Embedding choice is independent of the LLM provider choice: a user can
-- chat via Anthropic while embedding via local Ollama. One row per
-- (user, provider) so switching back to a previously-configured provider
-- keeps its model/key; exactly one row per user carries is_active = 1
-- (enforced by the service layer inside a transaction, same pattern as
-- user_provider_configs).
--
-- api_key_encrypted/api_key_nonce: AES-256-GCM, same scheme as
-- user_provider_configs. NULL key falls back to the user_provider_configs
-- row for the same provider string (key reuse — see
-- embedding_config_service::resolve_api_key).
CREATE TABLE user_embedding_configs (
    id                TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider          TEXT NOT NULL,
    model             TEXT NOT NULL,
    dimension         INTEGER NOT NULL,
    base_url          TEXT,
    api_key_encrypted BLOB,
    api_key_nonce     BLOB,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    UNIQUE (user_id, provider)
);

CREATE INDEX idx_user_embedding_configs_active
    ON user_embedding_configs (user_id, is_active);
