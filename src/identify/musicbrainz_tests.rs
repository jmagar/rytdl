use super::*;

#[test]
fn parse_recording_lookup_builds_retag_preview() {
    let json = br#"{
      "id": "recording-1",
      "title": "Harry Hood",
      "artist-credit": [
        {"name": "Phish", "artist": {"id": "artist-1", "name": "Phish"}}
      ],
      "releases": [
        {
          "id": "release-1",
          "title": "2024-08-29: Dick's Sporting Goods Park, Commerce City, CO, USA",
          "date": "2024-08-29",
          "release-group": {
            "id": "rg-1",
            "title": "2024-08-29: Dick's Sporting Goods Park, Commerce City, CO, USA",
            "primary-type": "Album",
            "secondary-types": ["Live"]
          },
          "media": [
            {
              "position": 1,
              "tracks": [
                {
                  "number": "15",
                  "title": "Harry Hood",
                  "recording": {"id": "recording-1"}
                }
              ]
            }
          ]
        }
      ]
    }"#;

    let preview = parse_recording_lookup(json, 0.999).expect("preview");

    assert_eq!(preview.confidence, 0.999);
    assert_eq!(preview.recording_id, "recording-1");
    assert_eq!(preview.recording_title, "Harry Hood");
    assert_eq!(preview.artist, "Phish");
    assert_eq!(preview.artists, vec!["Phish"]);
    assert_eq!(preview.release_id.as_deref(), Some("release-1"));
    assert_eq!(
        preview.release_title.as_deref(),
        Some("2024-08-29: Dick's Sporting Goods Park, Commerce City, CO, USA")
    );
    assert_eq!(preview.release_group_id.as_deref(), Some("rg-1"));
    assert_eq!(preview.release_type.as_deref(), Some("Album, Live"));
    assert_eq!(preview.release_date.as_deref(), Some("2024-08-29"));
    assert_eq!(preview.track_number.as_deref(), Some("15"));
    assert_eq!(
        preview.musicbrainz_url,
        "https://musicbrainz.org/recording/recording-1"
    );
}
