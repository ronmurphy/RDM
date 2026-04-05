/// A calculated window geometry.
#[derive(Debug, Clone)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Calculate master/stack layout for a set of tiled window IDs on a given output.
///
/// Layout:
///   - If 1 window: fills the entire output (minus outer gaps)
///   - If 2+ windows: master on the left (`master_ratio`), stack on the right
///   - Stack windows are evenly divided vertically
///
/// Returns (window_id, geometry) pairs in the same order as `window_ids`.
pub fn master_stack(
    window_ids: &[u32],
    output_x: i32,
    output_y: i32,
    output_w: i32,
    output_h: i32,
    master_ratio: f64,
    inner_gap: i32,
    outer_gap: i32,
) -> Vec<(u32, WindowGeometry)> {
    if window_ids.is_empty() {
        return Vec::new();
    }

    let usable_x = output_x + outer_gap;
    let usable_y = output_y + outer_gap;
    let usable_w = output_w - 2 * outer_gap;
    let usable_h = output_h - 2 * outer_gap;

    if window_ids.len() == 1 {
        return vec![(
            window_ids[0],
            WindowGeometry {
                x: usable_x,
                y: usable_y,
                width: usable_w,
                height: usable_h,
            },
        )];
    }

    // Master pane (left)
    let master_w = ((usable_w as f64) * master_ratio.clamp(0.2, 0.8)) as i32 - inner_gap / 2;
    let master_geo = WindowGeometry {
        x: usable_x,
        y: usable_y,
        width: master_w,
        height: usable_h,
    };

    // Stack pane (right)
    let stack_x = usable_x + master_w + inner_gap;
    let stack_w = usable_w - master_w - inner_gap;
    let stack_count = window_ids.len() - 1;
    let total_inner_gaps = (stack_count as i32 - 1).max(0) * inner_gap;
    let stack_item_h = (usable_h - total_inner_gaps) / stack_count as i32;

    let mut result = vec![(window_ids[0], master_geo)];

    for (i, &wid) in window_ids[1..].iter().enumerate() {
        let y = usable_y + (i as i32) * (stack_item_h + inner_gap);
        // Last stack window gets any remaining pixels
        let h = if i == stack_count - 1 {
            (usable_y + usable_h) - y
        } else {
            stack_item_h
        };
        result.push((wid, WindowGeometry {
            x: stack_x,
            y,
            width: stack_w,
            height: h,
        }));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_window_fills_screen() {
        let r = master_stack(&[1], 0, 0, 1920, 1080, 0.55, 8, 8);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].1.x, 8);
        assert_eq!(r[0].1.y, 8);
        assert_eq!(r[0].1.width, 1904);
        assert_eq!(r[0].1.height, 1064);
    }

    #[test]
    fn two_windows_master_stack() {
        let r = master_stack(&[1, 2], 0, 0, 1920, 1080, 0.5, 8, 8);
        assert_eq!(r.len(), 2);
        // Master gets left half
        assert_eq!(r[0].1.x, 8);
        assert!(r[0].1.width > 900 && r[0].1.width < 960);
        // Stack gets right half
        assert!(r[1].1.x > 960);
    }

    #[test]
    fn empty_returns_empty() {
        let r = master_stack(&[], 0, 0, 1920, 1080, 0.55, 8, 8);
        assert!(r.is_empty());
    }
}
