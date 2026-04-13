-- Multimodal messages store large base64 image data in `content`, which
-- exceeds PostgreSQL's tsvector size limit (1 MB).  Skip them for FTS.
ALTER TABLE messages
    DROP COLUMN content_tsv;

ALTER TABLE messages
    ADD COLUMN content_tsv tsvector GENERATED ALWAYS AS (
        to_tsvector('simple',
            CASE WHEN content IS NULL OR content LIKE '__multimodal__:%' THEN ''
                 ELSE content
            END
        )
    ) STORED;
