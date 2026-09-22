//! Reference ron-point baselines used by the risk weighter and the score
//! expectation. Source: the mahjong-helper Go `point_data.go` table.

/// Average non-dealer ron point baseline (reach, no ippatsu).
pub const RON_POINT_RIICHI: f64 = 5172.0;
/// Average non-dealer ron point with ippatsu.
pub const RON_POINT_RIICHI_IPPATSU: f64 = 7445.0;
/// Average non-dealer ron point for damaten.
pub const RON_POINT_DAMA: f64 = 4536.0;
/// Average non-dealer ron point for opened hands (dora-uncorrected).
pub const RON_POINT_OPEN: f64 = 3000.0;
/// Multiplier applied per dora to the open-hand ron point (first 3 dora).
pub const OPEN_DORA_MULTI_FIRST: f64 = 1.4;
/// Multiplier applied per dora to the open-hand ron point (4th and 5th dora).
pub const OPEN_DORA_MULTI_REST: f64 = 1.3;

/// Approximate ron point for an open hand given the count of dora it holds.
pub fn open_ron_point_with_dora(dora_count: u32) -> f64 {
    let mut point = RON_POINT_OPEN;
    let first = std::cmp::min(3, dora_count);
    for _ in 0..first {
        point *= OPEN_DORA_MULTI_FIRST;
    }
    let rest = dora_count.saturating_sub(3);
    let rest = std::cmp::min(2, rest);
    for _ in 0..rest {
        point *= OPEN_DORA_MULTI_REST;
    }
    point
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_dora_growth() {
        let p0 = open_ron_point_with_dora(0);
        let p1 = open_ron_point_with_dora(1);
        let p3 = open_ron_point_with_dora(3);
        let p5 = open_ron_point_with_dora(5);
        assert_eq!(p0, RON_POINT_OPEN);
        assert!((p1 - p0 * 1.4).abs() < 1e-6);
        assert!((p3 - p0 * 1.4_f64.powi(3)).abs() < 1e-6);
        assert!((p5 - p0 * 1.4_f64.powi(3) * 1.3_f64.powi(2)).abs() < 1e-6);
    }
}
