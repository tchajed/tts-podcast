-- Tigris renamed the public hostname from fly.storage.tigris.dev to t3.tigrisfiles.io.
-- The old hostname serves cached 403s for objects uploaded while the bucket was private.
UPDATE episodes
SET audio_url = REPLACE(audio_url, '.fly.storage.tigris.dev/', '.t3.tigrisfiles.io/')
WHERE audio_url LIKE '%.fly.storage.tigris.dev/%';

UPDATE episodes
SET image_url = REPLACE(image_url, '.fly.storage.tigris.dev/', '.t3.tigrisfiles.io/')
WHERE image_url LIKE '%.fly.storage.tigris.dev/%';
