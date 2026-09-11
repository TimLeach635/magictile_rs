//! Slice masks: bit masks selecting which slices (layers) a twist moves.

pub const SLICEMASK_1: i32 = 0x0001;
pub const SLICEMASK_2: i32 = 0x0002;
pub const SLICEMASK_3: i32 = 0x0004;
pub const SLICEMASK_4: i32 = 0x0008;
pub const SLICEMASK_5: i32 = 0x0010;
pub const SLICEMASK_6: i32 = 0x0020;
pub const SLICEMASK_7: i32 = 0x0040;
pub const SLICEMASK_8: i32 = 0x0080;
pub const SLICEMASK_9: i32 = 0x0100;
pub const SLICEMASK_10: i32 = 0x0200;

/// Slices are numbered from 1 (innermost) to 10.
pub fn slice_to_mask(slice: i32) -> i32 {
    if (1..=10).contains(&slice) { 1 << (slice - 1) } else { 0 }
}

/// For systolic puzzles, where "slices" mark the three pants directions (hexagon segments 1, 3, 5).
pub fn dir_seg_to_mask(dir_seg: i32) -> i32 {
    match dir_seg {
        1 => SLICEMASK_1,
        3 => SLICEMASK_2,
        5 => SLICEMASK_3,
        _ => 0,
    }
}

pub fn slice_to_dir_seg(slice: i32) -> i32 {
    match slice {
        1 => 1,
        2 => 3,
        3 => 5,
        _ => 0,
    }
}

pub fn mask_to_dir_seg(mask: i32) -> i32 {
    slice_to_dir_seg(mask_to_slice(mask))
}

/// The first slice in a mask (meant for single-slice masks on systolic puzzles), defaulting to 1.
pub fn mask_to_slice(mask: i32) -> i32 {
    mask_to_slices(mask).first().copied().unwrap_or(1)
}

pub fn mask_to_slices(mask: i32) -> Vec<i32> {
    (0..=10).filter(|&i| slice_to_mask(i) & mask != 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks() {
        assert_eq!(slice_to_mask(1), 1);
        assert_eq!(slice_to_mask(10), 0x200);
        assert_eq!(slice_to_mask(11), 0);
        assert_eq!(mask_to_slices(SLICEMASK_1 | SLICEMASK_3), vec![1, 3]);
        assert_eq!(mask_to_slice(0), 1);
        assert_eq!(mask_to_dir_seg(SLICEMASK_2), 3);
    }
}
