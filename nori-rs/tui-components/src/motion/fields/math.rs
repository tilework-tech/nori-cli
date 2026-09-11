//! Shared deterministic field math.

pub(in super::super) fn smooth(value: f64) -> f64 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

pub(super) fn hash(x: i32, y: i32, z: i32) -> f64 {
    let mut h = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263))
        .wrapping_add(z.wrapping_mul(1_274_126_177));
    h = (h ^ ((h as u32 >> 13) as i32)).wrapping_mul(1_274_126_177);
    f64::from((h ^ ((h as u32 >> 16) as i32)) as u32) / f64::from(u32::MAX)
}

pub(super) fn noise(x: f64, y: f64, t: f64) -> f64 {
    let mut value = 0.0;
    let sx = smooth(x - x.floor());
    let sy = smooth(y - y.floor());
    let st = smooth(t - t.floor());
    for dz in 0..2 {
        for dy in 0..2 {
            for dx in 0..2 {
                value += hash(
                    x.floor() as i32 + dx,
                    y.floor() as i32 + dy,
                    t.floor() as i32 + dz,
                ) * if dx == 0 { 1.0 - sx } else { sx }
                    * if dy == 0 { 1.0 - sy } else { sy }
                    * if dz == 0 { 1.0 - st } else { st };
            }
        }
    }
    value
}
