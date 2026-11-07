/// Fit measured panel content within eighty percent of the usable display.
pub fn panel_height(content_height: f64, available_height: f64) -> f64 {
    content_height.ceil().min((available_height * 0.8).floor())
}
