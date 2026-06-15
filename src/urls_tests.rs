use super::strip_mix_params;

#[test]
fn strips_mix_keeps_video() {
    let got = strip_mix_params(
        "https://www.youtube.com/watch?v=PLEQRIisP_Q&list=RDPLEQRIisP_Q&start_radio=1&pp=abc",
    );
    assert_eq!(got, "https://www.youtube.com/watch?v=PLEQRIisP_Q");
}

#[test]
fn leaves_real_playlist() {
    let u = "https://www.youtube.com/watch?v=abc&list=PLsomething";
    assert_eq!(strip_mix_params(u), u);
}

#[test]
fn leaves_plain_video() {
    let u = "https://www.youtube.com/watch?v=abc";
    assert_eq!(strip_mix_params(u), u);
}

#[test]
fn leaves_non_youtube() {
    let u = "https://vimeo.com/123456";
    assert_eq!(strip_mix_params(u), u);
}

#[test]
fn strips_rm_mix_prefix() {
    let got =
        strip_mix_params("https://www.youtube.com/watch?v=abc&list=RMabc&start_radio=1&pp=xyz");
    assert_eq!(got, "https://www.youtube.com/watch?v=abc");
}

#[test]
fn strips_wl_watch_later_prefix() {
    let got = strip_mix_params("https://www.youtube.com/watch?v=abc&list=WLfoo&index=3");
    assert_eq!(got, "https://www.youtube.com/watch?v=abc");
}

#[test]
fn handles_music_youtube_host() {
    let got = strip_mix_params("https://music.youtube.com/watch?v=abc&list=RDabc&start_radio=1");
    assert_eq!(got, "https://music.youtube.com/watch?v=abc");
}

#[test]
fn leaves_youtu_be_short_host() {
    // youtu.be carries the id in the path, not a `list=` param, so there is no
    // mix to strip and the URL passes through unchanged.
    let u = "https://youtu.be/abc";
    assert_eq!(strip_mix_params(u), u);
}

#[test]
fn keeps_unrelated_query_param() {
    // The mix params are stripped, but an unrelated `t=30` is preserved
    // (exercises the non-empty `kept` branch).
    let got = strip_mix_params("https://www.youtube.com/watch?v=abc&list=RDabc&t=30");
    assert_eq!(got, "https://www.youtube.com/watch?v=abc&t=30");
}

#[test]
fn leaves_non_url_string() {
    let u = "not a url";
    assert_eq!(strip_mix_params(u), u);
}
