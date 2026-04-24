-- Opaque per-turn provider state for round-tripping things like Anthropic
-- thinking-block sequences + signatures. Always NULL unless the producing
-- provider emitted an `LlmEvent::AssistantState`; consumed verbatim by the
-- same provider's request serializer on subsequent turns.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS provider_data JSONB;
