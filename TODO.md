# TODOs

## Needs user input

- Name the app, add a logo.
- Add a little audio boop between sections, the way AI Daily Brief does (but
  without copying their sound effect). Not sure where to get such a sound
  effect.

## Planned

- Add time remaining estimation. As each stage completes, we get more info
  for the later stages to make the estimate better (e.g., we need the
  content length to have any real estimate).

## Done

- Fix chapter timings and total duration: `mp3_duration` halts at the ID3
  tag inside the per-section silence MP3, so concatenated measurements
  returned ~1s. Per-chunk duration is now measured on the raw TTS output
  before appending silence, and the total is the sum of per-chunk durations.
- Read-only admin interface: `GET /api/v1/admin/{status,jobs,usage}` gated
  by `ADMIN_TOKEN`. TTS chunk progress is exposed per-episode in the feed
  view and per-job in `/admin/jobs`.
- AI cost tracking: `ai_usage` table records every AI call with tokens/chars.
  `tts-cli costs` queries the deployed server and prints a USD breakdown.
- API documentation for LLM consumption: [llms.txt](llms.txt) documents
  every endpoint + the SQLite schema.
- Time zone issue: investigated, only affects old episodes. All new
  timestamps are stored UTC and the frontend appends `Z` before formatting.
- Delete-episode button on the frontend episode page.
