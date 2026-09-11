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

/// Small frame-local cache of the eight lattice corners used by noise samples.
/// Direct mapping bounds memory; collisions only recompute a cell.
#[derive(Clone, Copy)]
struct NoiseCell {
    x: i32,
    y: i32,
    corners: [f64; 8],
}

pub(super) struct Noise {
    time: i32,
    weight: f64,
    cells: [Option<NoiseCell>; 16],
}

impl Noise {
    pub fn new(time: f64) -> Self {
        Self {
            time: time.floor() as i32,
            weight: smooth(time - time.floor()),
            cells: [None; 16],
        }
    }

    pub fn sample(&mut self, x: f64, y: f64) -> f64 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let slot = (ix.rem_euclid(4) + iy.rem_euclid(4) * 4) as usize;
        let cell = match self.cells[slot] {
            Some(cell) if cell.x == ix && cell.y == iy => cell,
            _ => {
                let cell = NoiseCell {
                    x: ix,
                    y: iy,
                    corners: std::array::from_fn(|index| {
                        let dx = (index % 2) as i32;
                        let dy = ((index / 2) % 2) as i32;
                        let dz = (index / 4) as i32;
                        hash(ix + dx, iy + dy, self.time + dz)
                    }),
                };
                self.cells[slot] = Some(cell);
                cell
            }
        };
        let sx = smooth(x - x.floor());
        let sy = smooth(y - y.floor());
        let st = self.weight;
        let mut value = 0.0;
        // Retain the original multiply/add order to preserve threshold decisions.
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    value += cell.corners[dz * 4 + dy * 2 + dx]
                        * if dx == 0 { 1.0 - sx } else { sx }
                        * if dy == 0 { 1.0 - sy } else { sy }
                        * if dz == 0 { 1.0 - st } else { st };
                }
            }
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn noise_cache_preserves_values_through_hits_collisions_and_negative_coordinates() {
        for time in [0.0, 1.25, 8639.9] {
            let mut noise = Noise::new(time);
            // The distant points deliberately collide in the direct-mapped cache.
            for (x, y) in [
                (0.2_f64, 0.7_f64),
                (0.4, 0.9),
                (4.2, 4.7),
                (0.2, 0.7),
                (-0.2, -4.7),
            ] {
                let sx = smooth(x - x.floor());
                let sy = smooth(y - y.floor());
                let st = smooth(time - time.floor());
                let mut expected = 0.0;
                for dz in 0..2 {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            expected += hash(
                                x.floor() as i32 + dx,
                                y.floor() as i32 + dy,
                                time.floor() as i32 + dz,
                            ) * if dx == 0 { 1.0 - sx } else { sx }
                                * if dy == 0 { 1.0 - sy } else { sy }
                                * if dz == 0 { 1.0 - st } else { st };
                        }
                    }
                }
                assert_eq!(noise.sample(x, y), expected);
            }
        }
    }
}
