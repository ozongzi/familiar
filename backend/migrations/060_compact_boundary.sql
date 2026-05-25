-- In-band compaction boundary.
--
-- Points at the last message that has been condensed into the latest
-- in-conversation summary message and therefore dropped from the loaded
-- window. `load_for_generation` returns messages with id > this value
-- (`messages[N..]`): the recent raw tail + the summary message (which carries
-- the condensed earlier context) + everything after it. NULL = the
-- conversation has never been compacted; load the full active branch.
ALTER TABLE conversations
    ADD COLUMN compact_drop_through_msg_id BIGINT;
