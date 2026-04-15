# TODOs

## Needs user input

- Name the app, add a logo.
- Add a little audio boop between sections, the way AI Daily Brief does (but
  without copying their sound effect). Not sure where to get such a sound
  effect.
- The published/created times on the website still don't make sense to me,
  there's some time zone issue I suspect. Retry times do look correct.
  (Investigated on 2026-04-15: all datetimes are stored as UTC via SQLite
  `datetime('now')` and the frontend appends `Z` before calling
  `toLocaleString`, so they render in the browser's local TZ. I don't see a
  bug from the code alone — need a concrete example of what looks wrong.)

## Planned

- Add a read-only management interface to get status of current jobs without
  arcane SQL commands. TTS chunk progress should be exposed in the frontend.
- Add time remaining estimation. As each stage completes, we get more info
  for the later stages to make the estimate better (e.g., we need the
  content length to have any real estimate).
- Add some mechanism for tracking AI costs (e.g., store token usage for each
  episode separately). In addition to tokens, I want an automated way (from
  the command line, not the web interface) to check actual dollar costs of
  each provider.
- Add API documentation, primarily for LLM consumption.
- Fix the chapter timings. For Spanner these are definitely incorrect.
  Probably the text chunking isn't properly following the headings in the
  first place.

## Miscellaneous

- Clean up code.
- Update documentation.
