//! traps.js::padOnGround uses only the prepared map collision at admission.
//! Its downward probe does not require a native soldier or authority world.
use three::{Matrix4, Ray, Vector3};
pub fn placement(
    matrix: [f64; 16],
    at: [f64; 3],
    collision: &bfe_content::collision::MapCollision,
) -> Result<[f64; 16], String> {
    if matrix.iter().chain(at.iter()).any(|v| !v.is_finite()) {
        return Err("invalid pad placement".into());
    }
    let mut local = Matrix4::default();
    local.make_translation(at[0], at[1], at[2]);
    local.premultiply(&Matrix4 { elements: matrix });
    let origin = Vector3::new(
        local.elements[12],
        local.elements[13] + 20.,
        local.elements[14],
    );
    let ray = Ray::new(origin, Vector3::new(0., -1., 0.));
    let mut nearest = 60.;
    let mut found = false;
    // Static buildings and terrain share the same world-space ray. The
    // closest hit below the probe supplies Y; authored X/Z and rotation stay.
    for (positions, index) in [
        (&collision.positions, &collision.indices),
        (&collision.terrain_positions, &collision.terrain_indices),
    ] {
        if positions.len() % 3 != 0 || index.len() % 3 != 0 {
            return Err("invalid pad collision stream".into());
        }
        let point = |i: u32| -> Result<Vector3, String> {
            let at = (i as usize).checked_mul(3).ok_or("pad index overflow")?;
            let p = positions
                .get(at..at + 3)
                .ok_or("pad index exceeds collision")?;
            Ok(Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64))
        };
        for triangle in index.as_chunks::<3>().0 {
            let mut hit = Vector3::default();
            if ray
                .intersect_triangle(
                    &point(triangle[0])?,
                    &point(triangle[1])?,
                    &point(triangle[2])?,
                    false,
                    &mut hit,
                )
                .is_some()
            {
                let distance = origin.y - hit.y;
                if distance >= 0. && distance <= nearest {
                    nearest = distance;
                    found = true;
                }
            }
        }
    }
    if found {
        local.elements[13] = origin.y - nearest;
    }
    Ok(local.elements)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_offset_is_grounded_and_a_miss_retains_its_height() {
        let floats = |v: &[f32]| v.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
        let words = |v: &[u32]| v.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
        let collision = bfe_content::collision::MapCollision::load(
            &[],
            &[],
            &[],
            &floats(&[0., 4., 0., 100., 4., 0., 0., 4., 100.]),
            &words(&[0, 1, 2]),
        )
        .unwrap();
        let mut root = Matrix4::default();
        root.elements[12] = 10.;
        root.elements[14] = 10.;
        let grounded = placement(root.elements, [1., 0., 1.], &collision).unwrap();
        assert_eq!(&grounded[12..15], &[11., 4., 11.]);
        let miss = placement(root.elements, [-100., 2., -100.], &collision).unwrap();
        assert_eq!(miss[13], 2.);
    }
}
