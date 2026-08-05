//! Stable context-occupancy display shared by terminal surfaces.

/// Format prompt occupancy against a declared context window.
///
/// Ratios above 100% and non-finite ratios stay visible: they can expose an
/// incorrect catalog window and must not be disguised by clamping. When the
/// formatted digits are `0` and the true ratio is above zero, the established
/// `<` marker keeps that measured occupancy visible.
pub fn format_context_percent(prompt_tokens: u64, context_window: u64) -> String {
    let ratio = prompt_tokens as f64 / context_window as f64 * 100.0;
    let digits = format!("{ratio:.0}");
    if digits == "0" && ratio > 0.0 {
        "<1%".to_owned()
    } else {
        format!("{digits}%")
    }
}
