use std::path::Path;

use anyhow::{bail, Context, Result};
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey, Tag};
use serde::Serialize;

use super::RetagPreview;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TagWriteResult {
    pub written: bool,
    pub fields: Vec<String>,
}

pub(crate) fn write_retag_preview(path: &Path, preview: &RetagPreview) -> Result<TagWriteResult> {
    let mut tagged_file = lofty::read_from_path(path)
        .with_context(|| format!("read audio tags from {}", path.display()))?;
    let tag_type = tagged_file.primary_tag_type();
    if !tagged_file.tag_support(tag_type).is_writable() {
        bail!(
            "{} does not support writable {:?} tags",
            path.display(),
            tag_type
        );
    }
    if tagged_file.primary_tag().is_none() {
        tagged_file.insert_tag(Tag::new(tag_type));
    }
    let fields = {
        let tag = tagged_file
            .primary_tag_mut()
            .context("audio file has no writable tag")?;
        apply_preview_to_tag(tag, preview)
    };
    tagged_file
        .save_to_path(path, WriteOptions::default())
        .with_context(|| format!("write audio tags to {}", path.display()))?;
    Ok(TagWriteResult {
        written: true,
        fields,
    })
}

pub(crate) fn apply_preview_to_tag(tag: &mut Tag, preview: &RetagPreview) -> Vec<String> {
    let mut fields = Vec::new();

    set_accessor(&mut fields, "artist", || {
        tag.set_artist(preview.artist.clone());
    });
    set_accessor(&mut fields, "title", || {
        tag.set_title(preview.recording_title.clone());
    });
    if let Some(album) = preview.release_title.as_deref() {
        set_accessor(&mut fields, "album", || {
            tag.set_album(album.to_string());
        });
    }
    set_text(
        tag,
        &mut fields,
        ItemKey::AlbumArtist,
        "album_artist",
        &preview.artist,
    );
    if let Some(date) = preview.release_date.as_deref() {
        set_text(tag, &mut fields, ItemKey::RecordingDate, "date", date);
        set_text(tag, &mut fields, ItemKey::ReleaseDate, "release_date", date);
    }
    if let Some(track) = preview.track_number.as_deref() {
        set_text(
            tag,
            &mut fields,
            ItemKey::TrackNumber,
            "track_number",
            track,
        );
    }
    set_text(
        tag,
        &mut fields,
        ItemKey::MusicBrainzRecordingId,
        "musicbrainz_recording_id",
        &preview.recording_id,
    );
    if let Some(release_id) = preview.release_id.as_deref() {
        set_text(
            tag,
            &mut fields,
            ItemKey::MusicBrainzReleaseId,
            "musicbrainz_release_id",
            release_id,
        );
    }
    if let Some(group_id) = preview.release_group_id.as_deref() {
        set_text(
            tag,
            &mut fields,
            ItemKey::MusicBrainzReleaseGroupId,
            "musicbrainz_release_group_id",
            group_id,
        );
    }
    if let Some(release_type) = preview.release_type.as_deref() {
        set_text(
            tag,
            &mut fields,
            ItemKey::MusicBrainzReleaseType,
            "musicbrainz_release_type",
            release_type,
        );
    }

    fields
}

fn set_accessor(fields: &mut Vec<String>, field: &str, set: impl FnOnce()) {
    set();
    fields.push(field.to_string());
}

fn set_text(tag: &mut Tag, fields: &mut Vec<String>, key: ItemKey, field: &str, value: &str) {
    tag.insert_text(key, value.to_string());
    fields.push(field.to_string());
}

#[cfg(test)]
#[path = "tagger_tests.rs"]
mod tests;
