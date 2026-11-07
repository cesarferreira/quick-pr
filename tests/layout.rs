use quick_pr::layout::panel_height;

#[test]
fn fits_content_and_shrinks_when_there_are_fewer_prs() {
    assert_eq!(panel_height(620., 1000.), 620.);
    assert_eq!(panel_height(340., 1000.), 340.);
    assert_eq!(panel_height(250., 1000.), 250.);
}

#[test]
fn caps_long_lists_at_eighty_percent_of_the_current_display() {
    assert_eq!(panel_height(1600., 1000.), 800.);
    assert_eq!(panel_height(1600., 700.), 560.);
    assert_eq!(panel_height(560., 700.), 560.);
}

#[test]
fn rounds_content_up_without_exceeding_the_screen_cap() {
    assert_eq!(panel_height(340.2, 1000.), 341.);
    assert_eq!(panel_height(900., 777.), 621.);
}
