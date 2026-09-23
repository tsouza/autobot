use autobot_devtools::banner::render;

const README: &str = include_str!("../../../README.md");
const BANNER: &str = include_str!("../../../assets/banner.txt");
const SVG: &str = include_str!("../../../assets/banner.svg");

/// The line the README opens with.
const IMAGE: &str = r#"<img src="assets/banner.svg" width="480" alt="AutoBot">"#;

#[test]
fn the_banner_image_is_the_rendering_of_the_banner_text() {
    assert_eq!(render(BANNER).ok().as_deref(), Some(SVG));
}

#[test]
fn an_edited_banner_text_is_a_difference() {
    let edited = BANNER.replacen('■', "█", 1);
    assert_ne!(edited, BANNER);
    assert_ne!(render(&edited).ok().as_deref(), Some(SVG));
}

#[test]
fn an_edited_banner_image_is_a_difference() {
    let edited = SVG.replacen("h2v2", "h2v3", 1);
    assert_ne!(edited, SVG);
    assert_ne!(render(BANNER).ok().as_deref(), Some(edited.as_str()));
}

#[test]
fn an_unknown_character_in_the_banner_text_is_an_error() {
    assert!(render(&BANNER.replacen('■', "#", 1)).is_err());
}

#[test]
fn the_readme_opens_with_the_banner_image() {
    assert_eq!(README.lines().next(), Some(IMAGE));
}
