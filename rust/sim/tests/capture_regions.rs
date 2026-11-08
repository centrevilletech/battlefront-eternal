//! World capture, control, and death region geometry tests ported from
//! `tests/capture-regions.test.js`.
//!
//! These cases exercise `bfe_sim::battle::regions::{build_region, in_region,
//! region_desc, DeathRegions}` across sphere, cylinder, and box regions with
//! non-uniform transforms and rotations.
//!
//! Coverage includes:
//! - Sphere radius computed as `hypot3(size.x, size.y, size.z)` independent of
//!   the transform's scale axes (e.g. Tatooine Dune Sea's `HOMESTEAD` sized
//!   `[0.1, 8.15, 0.1]` has radius `8.151m`, not `0.1m`).
//! - Cylinder radius computed as `hypot(size.x * scale.x, size.z * scale.z)`
//!   with `size.y * scale.y` as half-height (e.g. Tatooine's `Sarlac01` pit
//!   sized `[2.135, 6.056, 8.158]` has radius `8.43m` and half-height `6.06m`).
//! - Box and cylinder half-extents scaled by the transform's basis axis lengths
//!   while keeping unit-length local axes for inverse point containment checks.

use bfe_sim::battle::regions::{build_region, in_region, region_desc, DeathRegions};
use bfe_sim::gen::RegionType;
use three::{Matrix4, Vector3};

/// Builds a 16-element translation matrix array at `(x, y, z)`.
fn translation_matrix(x: f64, y: f64, z: f64) -> [f64; 16] {
    let mut m = Matrix4::default();
    m.make_translation(x, y, z);
    m.elements
}

// Mirrors `tests/capture-regions.test.js`:
// "a sphere's radius is the length of its SIZE, whatever its transform"
#[test]
fn sphere_radius_is_length_of_size_whatever_its_transform() {
    // Tatooine Dune Sea HOMESTEAD capture sphere at (0, 0.4, 0) with size [0.1, 8.15, 0.1].
    let mat = translation_matrix(0.0, 0.4, 0.0);
    let desc = region_desc(RegionType::Sphere, Some(&mat), None, [0.1, 8.15, 0.1]);
    let post = build_region(&desc);

    // Points within 8.151m of (0, 0.4, 0) are inside; 8.2m out is outside.
    assert!(in_region(&post, &Vector3::new(1.0, 0.0, 1.0)));
    assert!(in_region(&post, &Vector3::new(5.0, 0.0, -5.0)));
    assert!(in_region(&post, &Vector3::new(0.0, 0.4 + 8.1, 0.0)));
    assert!(!in_region(&post, &Vector3::new(8.2, 0.4, 0.0)));

    // Scaling the transform matrix by 3x leaves the sphere's radius unchanged.
    let mut scale_m = Matrix4::default();
    scale_m.make_scale(3.0, 3.0, 3.0);
    let scaled_desc = region_desc(
        RegionType::Sphere,
        Some(&scale_m.elements),
        None,
        [0.1, 8.15, 0.1],
    );
    let scaled = build_region(&scaled_desc);
    let expected_r = v8math::hypot3(0.1, 8.15, 0.1);
    assert!((scaled.radius - expected_r).abs() < 1e-12);
    assert!(!in_region(&scaled, &Vector3::new(8.2, 0.0, 0.0)));
}

// Mirrors `tests/capture-regions.test.js`:
// "a cylinder's radius is the length of its SIZE's x and z, its half height y"
#[test]
fn cylinder_radius_is_length_of_size_x_and_z_and_half_height_is_y() {
    // Capture cylinder at (0, 0.4, 0) with size [0.1, 5.0, 16.0] -> radius ~16.0m, half-height 5.0m.
    let mat = translation_matrix(0.0, 0.4, 0.0);
    let desc = region_desc(RegionType::Cylinder, Some(&mat), None, [0.1, 5.0, 16.0]);
    let post = build_region(&desc);

    assert!(in_region(&post, &Vector3::new(15.0, 0.0, 0.0)));
    assert!(in_region(&post, &Vector3::new(0.0, 5.3, 0.0)));
    assert!(!in_region(&post, &Vector3::new(0.0, 5.5, 0.0)));
    assert!(!in_region(&post, &Vector3::new(16.1, 0.0, 0.0)));

    // Tatooine's sarlacc pit (Sarlac01) at (-261, 3.7, 92.6) with size [2.135, 6.056, 8.158].
    let pit_mat = translation_matrix(-261.0, 3.7, 92.6);
    let pit_desc = region_desc(
        RegionType::Cylinder,
        Some(&pit_mat),
        None,
        [2.135, 6.056, 8.158],
    );
    let pit = build_region(&pit_desc);
    assert_eq!(format!("{:.2}", pit.radius), "8.43");
    assert_eq!(
        pit.half.map(|x| format!("{x:.2}")),
        ["8.43", "6.06", "8.43"]
    );
    assert!(in_region(&pit, &Vector3::new(-256.0, 5.0, 92.6)));
    assert!(!in_region(&pit, &Vector3::new(-251.0, 5.0, 92.6)));
    assert!(!in_region(&pit, &Vector3::new(-261.0, 10.0, 92.6)));

    // Also verify `DeathRegions` admits scripted `Sarlac01` and `kill_y` floor threshold.
    let death = DeathRegions::new(
        -50.0,
        &[(Some("Sarlac01".to_string()), pit_desc)],
        &["sarlac01".to_string()],
    );
    assert!(death.contains(&Vector3::new(-256.0, 5.0, 92.6)));
    assert!(death.contains(&Vector3::new(0.0, -55.0, 0.0)));
    assert!(!death.contains(&Vector3::new(-251.0, 5.0, 92.6)));
}

// Mirrors `tests/capture-regions.test.js`:
// "box and cylinder sizes scale with the transform's axes; the axes are unit for the test"
#[test]
fn box_and_cylinder_sizes_scale_with_transform_axes_and_use_unit_axes_for_containment() {
    // Box at (10, 0, 0), rotated +90 deg around Y, scaled by (2, 1, 1), size [3, 1, 1].
    let mut matrix = Matrix4::default();
    matrix.make_translation(10.0, 0.0, 0.0);
    let mut turn = Matrix4::default();
    turn.make_rotation_y(std::f64::consts::FRAC_PI_2);
    let mut scale = Matrix4::default();
    scale.make_scale(2.0, 1.0, 1.0);
    matrix.multiply(&turn).multiply(&scale);

    let box_desc = region_desc(
        RegionType::Box,
        Some(&matrix.elements),
        None,
        [3.0, 1.0, 1.0],
    );
    let built_box = build_region(&box_desc);
    for (got, want) in built_box.half.iter().zip([6.0, 1.0, 1.0]) {
        assert!((got - want).abs() < 1e-12);
    }
    // Local X (6m half-extent) aligns with world Z after the quarter turn around Y.
    assert!(in_region(&built_box, &Vector3::new(10.0, 0.0, 5.5)));
    assert!(!in_region(&built_box, &Vector3::new(10.0, 0.0, 6.5)));
    assert!(!in_region(&built_box, &Vector3::new(11.5, 0.0, 0.0)));

    // Cylinder rotated +90 deg around Z and scaled by (1, 2, 1), size [3, 2, 4].
    let mut tilted = Matrix4::default();
    tilted.make_rotation_z(std::f64::consts::FRAC_PI_2);
    let mut cyl_scale = Matrix4::default();
    cyl_scale.make_scale(1.0, 2.0, 1.0);
    tilted.multiply(&cyl_scale);

    let cyl_desc = region_desc(
        RegionType::Cylinder,
        Some(&tilted.elements),
        None,
        [3.0, 2.0, 4.0],
    );
    let cylinder = build_region(&cyl_desc);
    for (got, want) in cylinder.half.iter().zip([5.0, 4.0, 5.0]) {
        assert!((got - want).abs() < 1e-12);
    }
    // Cylinder axis is world X after the Z tilt: 4m half-length along X, 5m radius in YZ.
    assert!(in_region(&cylinder, &Vector3::new(3.9, 4.9, 0.0)));
    assert!(!in_region(&cylinder, &Vector3::new(4.1, 0.0, 0.0)));
    assert!(!in_region(&cylinder, &Vector3::new(0.0, 5.1, 0.0)));
}
