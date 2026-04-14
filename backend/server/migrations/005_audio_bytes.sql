-- Track MP3 byte size so the RSS enclosure can report a correct length.
ALTER TABLE episodes ADD COLUMN audio_bytes INTEGER;
