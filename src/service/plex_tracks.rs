//! Projection of downloader results into the Plex-owned input DTO, extracted
//! from `service.rs`.

use crate::downloader::ItemResult;

/// Map download results into the Plex-owned [`crate::plex::PlexTrackInput`] DTO.
///
/// One DTO per downloaded *audio* file, in result/file order. The display title
/// is resolved from the file's own title, falling back to the item title, then
/// the file stem; files whose resolved title is empty are dropped. The uploader
/// falls back from the file's uploader to the item's. De-duplication of equal
/// (title, uploader) pairs is left to the Plex layer so this mapper stays a pure
/// projection of the downloader model.
pub(super) fn plex_track_inputs(results: &[ItemResult]) -> Vec<crate::plex::PlexTrackInput> {
    let mut tracks = Vec::new();
    for result in results {
        for file in &result.files {
            if file.kind != "audio" {
                continue;
            }
            let title = file
                .title
                .as_deref()
                .or(result.title.as_deref())
                .or_else(|| file.path.file_stem().and_then(|s| s.to_str()))
                .unwrap_or("")
                .trim();
            if title.is_empty() {
                continue;
            }
            let uploader = file.uploader.clone().or_else(|| result.uploader.clone());
            tracks.push(crate::plex::PlexTrackInput {
                title: title.to_string(),
                uploader,
            });
        }
    }
    tracks
}
