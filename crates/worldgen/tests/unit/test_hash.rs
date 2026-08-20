use worldgen::{Domain, derive};

/// Every domain, so a new one cannot be added without deciding whether it
/// belongs in the independence checks below.
const ALL: [Domain; 5] = [
    Domain::Height,
    Domain::RegionJitter,
    Domain::RegionBiome,
    Domain::Stray,
    Domain::Site,
];

#[test]
fn the_same_inputs_always_give_the_same_value() {
    assert_eq!(
        derive(7, Domain::Height, -3, 9),
        derive(7, Domain::Height, -3, 9)
    );
}

#[test]
fn two_domains_at_one_coordinate_disagree() {
    // The defect this replaces: without a domain tag, elevation and moisture
    // sampled at the same tile were the same number.
    for (i, &a) in ALL.iter().enumerate() {
        for &b in &ALL[i + 1..] {
            assert_ne!(derive(7, a, 4, 4), derive(7, b, 4, 4), "{a:?} vs {b:?}");
        }
    }
}

#[test]
fn swapping_the_axes_changes_the_value() {
    // A hash that folded both axes in the same way would make the world
    // mirror-symmetric about the diagonal.
    assert_ne!(
        derive(7, Domain::Height, 5, 11),
        derive(7, Domain::Height, 11, 5)
    );
}

#[test]
fn negative_coordinates_are_distinct_from_positive_ones() {
    assert_ne!(
        derive(7, Domain::Height, -5, -5),
        derive(7, Domain::Height, 5, 5)
    );
    assert_ne!(
        derive(7, Domain::Height, -5, 5),
        derive(7, Domain::Height, 5, -5)
    );
}

#[test]
fn the_extremes_of_the_coordinate_range_do_not_panic() {
    for x in [i32::MIN, -1, 0, 1, i32::MAX] {
        for y in [i32::MIN, -1, 0, 1, i32::MAX] {
            let _ = derive(7, Domain::Site, x, y);
        }
    }
}

#[test]
fn two_seeds_disagree_at_one_coordinate() {
    assert_ne!(
        derive(7, Domain::Height, 0, 0),
        derive(8, Domain::Height, 0, 0)
    );
}

/// Correlation between two domains over a grid, as Pearson's r on the low bits.
fn correlation(a: Domain, b: Domain) -> f64 {
    const SIDE: i32 = 64;
    let sample = |domain, x, y| (derive(7, domain, x, y) >> 11) as f64;
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for y in 0..SIDE {
        for x in 0..SIDE {
            xs.push(sample(a, x, y));
            ys.push(sample(b, x, y));
        }
    }
    let n = xs.len() as f64;
    let mean = |v: &[f64]| v.iter().sum::<f64>() / n;
    let (mx, my) = (mean(&xs), mean(&ys));
    let cov: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let dev = |v: &[f64], m: f64| v.iter().map(|x| (x - m).powi(2)).sum::<f64>().sqrt();
    cov / (dev(&xs, mx) * dev(&ys, my))
}

#[test]
fn domains_are_statistically_independent() {
    // Distinctness alone would pass with two domains that differ by a constant.
    // Independence is the property callers actually rely on.
    for (i, &a) in ALL.iter().enumerate() {
        for &b in &ALL[i + 1..] {
            let r = correlation(a, b);
            assert!(r.abs() < 0.05, "{a:?} vs {b:?} correlate at {r}");
        }
    }
}

#[test]
fn one_domain_is_uncorrelated_with_its_own_neighbour() {
    // A position hash must not vary smoothly. Smoothness is what noise is for,
    // and it is layered on top of this.
    const SIDE: i32 = 64;
    let sample = |x, y| (derive(7, Domain::Height, x, y) >> 11) as f64;
    let mut here = Vec::new();
    let mut east = Vec::new();
    for y in 0..SIDE {
        for x in 0..SIDE {
            here.push(sample(x, y));
            east.push(sample(x + 1, y));
        }
    }
    let n = here.len() as f64;
    let mean = |v: &[f64]| v.iter().sum::<f64>() / n;
    let (mh, me) = (mean(&here), mean(&east));
    let cov: f64 = here
        .iter()
        .zip(&east)
        .map(|(a, b)| (a - mh) * (b - me))
        .sum();
    let dev = |v: &[f64], m: f64| v.iter().map(|x| (x - m).powi(2)).sum::<f64>().sqrt();
    let r = cov / (dev(&here, mh) * dev(&east, me));
    assert!(r.abs() < 0.05, "adjacent tiles correlate at {r}");
}

#[test]
fn the_low_bits_are_usable_as_a_percentage() {
    // Callers reach for `% 100`, so a biased low byte would skew every roll.
    const SAMPLES: i32 = 20_000;
    let mut buckets = [0u32; 10];
    for i in 0..SAMPLES {
        let roll = derive(7, Domain::Stray, i, -i) % 100;
        buckets[(roll / 10) as usize] += 1;
    }
    let expected = f64::from(SAMPLES) / 10.0;
    for (decile, &count) in buckets.iter().enumerate() {
        let error = (f64::from(count) - expected).abs() / expected;
        assert!(error < 0.1, "decile {decile} off by {error}");
    }
}

#[test]
fn the_hash_is_pinned() {
    // Changing this value changes every world ever generated. If this test
    // fails, that is the question to answer, not the number to update.
    assert_eq!(
        derive(0x4D61_7272_6F77, Domain::Height, 0, 0),
        0xAB0B_69AE_3801_D924
    );
}

#[test]
fn a_variant_is_mixed_in_as_its_own_input() {
    // The trap this avoids: xor-ing a class id into a coordinate makes class A at
    // one cell bit-identical to class B at a shifted cell, so two classes place
    // their sites in lockstep. A multiplied input cannot collide that way.
    use worldgen::derive_with;
    for shift in 1..8i32 {
        assert_ne!(
            derive_with(7, Domain::Site, 0, shift, 0),
            derive_with(7, Domain::Site, shift as u64, 0, 0),
            "variant {shift} collided with a shifted coordinate"
        );
    }
}

#[test]
fn variants_of_one_domain_are_independent() {
    use worldgen::derive_with;
    const SIDE: i32 = 48;
    let sample = |variant, x, y| (derive_with(7, Domain::Site, variant, x, y) >> 11) as f64;
    for (a, b) in [(0u64, 1u64), (1, 2), (0, 7), (3, 9)] {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for y in 0..SIDE {
            for x in 0..SIDE {
                xs.push(sample(a, x, y));
                ys.push(sample(b, x, y));
            }
        }
        let n = xs.len() as f64;
        let mean = |v: &[f64]| v.iter().sum::<f64>() / n;
        let (mx, my) = (mean(&xs), mean(&ys));
        let cov: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
        let dev = |v: &[f64], m: f64| v.iter().map(|x| (x - m).powi(2)).sum::<f64>().sqrt();
        let r = cov / (dev(&xs, mx) * dev(&ys, my));
        assert!(r.abs() < 0.05, "variants {a} and {b} correlate at {r}");
    }
}

#[test]
fn derive_is_the_zero_variant() {
    use worldgen::derive_with;
    assert_eq!(
        derive(7, Domain::Site, 3, -4),
        derive_with(7, Domain::Site, 0, 3, -4)
    );
}
