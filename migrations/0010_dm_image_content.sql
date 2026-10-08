-- Image/file DMs may use an empty caption. Text DMs still require content.
ALTER TABLE direct_messages DROP CONSTRAINT IF EXISTS direct_messages_content_not_blank;

ALTER TABLE direct_messages
    ADD CONSTRAINT direct_messages_content_when_text CHECK (
        message_type IN ('image', 'file') OR btrim(content) <> ''
    );
