# MusicBrainz / AcoustID metadata plan

## Goal

Improve downloaded audio tags with canonical recording metadata instead of
guessing from YouTube titles. The download path should keep working if external
matching is unavailable, slow, ambiguous, or not configured.

## External constraints

- MusicBrainz requires a meaningful `User-Agent` and limits clients to one
  request per second.
- AcoustID lookup requires an application API key, full-file duration, and a
  Chromaprint fingerprint.
- AcoustID can return MusicBrainz recording IDs with `meta=recordingids`; richer
  release metadata should then come from MusicBrainz.
- This repo does not currently have `fpcalc` on the host, so fingerprinting
  needs either a bundled resolver similar to yt-dlp/ffmpeg or an in-process
  Rust path.

## Recommended implementation

1. Add an opt-in retagging mode:
   - `YTDLP_ACOUSTID_CLIENT_KEY`
   - `YTDLP_MUSICBRAINZ_CONTACT`
   - `YTDLP_CANONICAL_METADATA=0|1` (default off until proven)
2. After audio extraction, fingerprint each final audio file before transfer.
3. Query AcoustID `/v2/lookup` with `meta=recordingids` and require a high score.
4. Rate-limit MusicBrainz lookups globally to one request per second.
5. Fetch candidate recordings from MusicBrainz with artist/release metadata.
6. Score candidates against existing yt-dlp metadata:
   - AcoustID score
   - duration delta
   - normalized artist/title similarity
   - release availability
7. Only write canonical tags above a confidence threshold; otherwise preserve
   yt-dlp tags and report the ambiguity.
8. Use `lofty` for tag writes once a match is accepted.

## Candidate crates/tools

- `musicbrainz_rs` for MusicBrainz API access; it has sync/async and optional
  rate-limit features.
- AcoustID can be a small direct HTTP client; there is no obvious maintained
  Rust AcoustID API crate.
- `rusty-chromaprint` or `chromaprint-next` can generate fingerprints in Rust,
  but both need decoded PCM. Pairing with `symphonia` is the likely pure-Rust
  route.
- `fpcalc` would be simpler operationally, but it is not installed here today
  and would need cross-platform bootstrap/download support.
- `lofty` is the likely tag-writing crate for MP3/M4A/FLAC/Opus metadata.

## First safe milestone

Add a read-only `youtube_identify` tool that fingerprints local staged/downloaded
audio and returns candidate MusicBrainz matches with confidence reasons. Once
that is trustworthy, add opt-in tag writing to `youtube_download`.

Sources:
- https://musicbrainz.org/doc/MusicBrainz_API
- https://acoustid.org/webservice
- https://docs.rs/musicbrainz_rs
