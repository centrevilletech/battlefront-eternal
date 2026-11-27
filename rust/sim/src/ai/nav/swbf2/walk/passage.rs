//! Short polylines through the opening of an indoor door.
use super::{clear, length, BarrierSet, Survey, Vec3};

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::ai::nav::swbf2) struct Bends {
    points: [Vec3; 6],
    len: u8,
}

impl Bends {
    pub(super) fn one(bend: Option<Vec3>) -> Self {
        let mut path = Self::default();
        if let Some(point) = bend {
            path.points[0] = point;
            path.len = 1;
        }
        path
    }

    pub(in crate::ai::nav::swbf2) fn points(&self) -> &[Vec3] {
        &self.points[..usize::from(self.len)]
    }
}

pub(super) fn passage(
    survey: &mut impl Survey,
    barriers: &mut BarrierSet,
    from: Vec3,
    door: Vec3,
    to: Vec3,
    charge: &mut impl FnMut() -> Result<(), String>,
) -> Result<Option<Bends>, String> {
    // The doorway and samples on both approaches form a small visibility
    // graph. All links use the same mover and enabled barriers as other legs.
    let mut sites = [Vec3::default(); 56];
    sites[0] = from;
    sites[1] = to;
    let mut count = 2;
    for (a, b) in [(from, door), (door, to)] {
        let dx = b.x - a.x;
        let dz = b.z - a.z;
        let norm = v8math::max(v8math::hypot(dx, dz), 0.001);
        for t in [0.25, 0.5, 0.75, 1.] {
            for offset in [0., -2.5, 2.5, -5., 5., -8., 8.] {
                if count == sites.len() {
                    break;
                }
                charge()?;
                let guess = Vec3::new(
                    a.x + dx * t - dz * offset / norm,
                    a.y + (b.y - a.y) * t,
                    a.z + dz * t + dx * offset / norm,
                );
                if let Some(p) = survey.floor(guess) {
                    if sites[..count].iter().all(|&q| length(p, q) > 0.5)
                        && barriers.point_clear(p, 0.55, 1).0
                    {
                        sites[count] = p;
                        count += 1;
                    }
                }
            }
        }
    }
    let mut distance = [f64::INFINITY; 56];
    let mut parent = [usize::MAX; 56];
    let mut done = [false; 56];
    let mut steps = [0; 56];
    distance[0] = 0.;
    while let Some(i) = (0..count)
        .filter(|&i| !done[i] && distance[i].is_finite())
        .min_by(|&i, &j| distance[i].total_cmp(&distance[j]))
    {
        if i == 1 {
            let mut path = Bends::default();
            let mut at = parent[1];
            while at != 0 {
                path.points[usize::from(path.len)] = sites[at];
                path.len += 1;
                at = parent[at];
            }
            path.points[..usize::from(path.len)].reverse();
            return Ok(Some(path));
        }
        done[i] = true;
        for j in 1..count {
            if done[j] || (steps[i] >= 6 && j != 1) {
                continue;
            }
            let (p, q) = (sites[i], sites[j]);
            let d = distance[i] + length(p, q);
            if d >= distance[j] || !clear(barriers, p, q, 0.55, 1) {
                continue;
            }
            charge()?;
            if survey.crosses(p, q, 0) {
                distance[j] = d;
                parent[j] = i;
                steps[j] = steps[i] + 1;
            }
        }
    }
    deck_passage(survey, barriers, from, door, to, charge)
}

/// Search a bounded patch of deck when offset samples miss a corner. Keep the
/// door on the route even where a second opening reaches the same junction.
fn deck_passage(
    survey: &mut impl Survey,
    barriers: &mut BarrierSet,
    from: Vec3,
    door: Vec3,
    to: Vec3,
    charge: &mut impl FnMut() -> Result<(), String>,
) -> Result<Option<Bends>, String> {
    let Some(threshold) = survey.floor(Vec3::new(door.x, (from.y + to.y) * 0.5, door.z)) else {
        return Ok(None);
    };
    if !barriers.point_clear(threshold, 0.55, 1).0 {
        return Ok(None);
    }
    let mut path = Bends::default();
    for (a, b) in [(from, threshold), (threshold, to)] {
        charge()?;
        if !(clear(barriers, a, b, 0.55, 1) && survey.crosses(a, b, 0)) {
            let Some(points) = deck_detour(survey, barriers, a, b, charge)? else {
                return Ok(None);
            };
            for p in points {
                if usize::from(path.len) == path.points.len() {
                    return Ok(None);
                }
                path.points[usize::from(path.len)] = p;
                path.len += 1;
            }
        }
        if b == threshold {
            if usize::from(path.len) == path.points.len() {
                return Ok(None);
            }
            path.points[usize::from(path.len)] = b;
            path.len += 1;
        }
    }
    Ok(Some(path))
}

fn deck_detour(
    survey: &mut impl Survey,
    barriers: &mut BarrierSet,
    a: Vec3,
    b: Vec3,
    charge: &mut impl FnMut() -> Result<(), String>,
) -> Result<Option<Vec<Vec3>>, String> {
    const MARGIN: usize = 8;
    let width = (b.x - a.x).abs().ceil() as usize;
    let depth = (b.z - a.z).abs().ceil() as usize;
    if width > 48 || depth > 48 {
        return Ok(None);
    }
    let (width, depth) = (width + 2 * MARGIN + 1, depth + 2 * MARGIN + 1);
    let (x0, z0) = (a.x.min(b.x) - MARGIN as f64, a.z.min(b.z) - MARGIN as f64);
    let index = |p: Vec3| (p.z - z0).round() as usize * width + (p.x - x0).round() as usize;
    let (start, goal) = (index(a), index(b));
    if start == goal {
        return Ok(None);
    }
    let mut sites = Vec::with_capacity(width * depth);
    for z in 0..depth {
        for x in 0..width {
            charge()?;
            let p = Vec3::new(x0 + x as f64, (a.y + b.y) * 0.5, z0 + z as f64);
            sites.push(if barriers.point_clear(p, 0.55, 1).0 {
                survey.floor(p)
            } else {
                None
            });
        }
    }
    sites[start] = Some(a);
    sites[goal] = Some(b);
    let mut costs = vec![u32::MAX; sites.len()];
    let mut parent = vec![usize::MAX; sites.len()];
    let mut open = std::collections::BinaryHeap::new();
    let estimate = |i: usize| {
        let dx = (i % width).abs_diff(goal % width) as u32;
        let dz = (i / width).abs_diff(goal / width) as u32;
        10 * dx.max(dz) + 4 * dx.min(dz)
    };
    costs[start] = 0;
    open.push(std::cmp::Reverse((estimate(start), 0, start)));
    while let Some(std::cmp::Reverse((_, cost, i))) = open.pop() {
        if cost != costs[i] {
            continue;
        }
        if i == goal {
            break;
        }
        let p = sites[i].unwrap();
        for (dx, dz) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let (Some(x), Some(z)) = (
                (i % width).checked_add_signed(dx),
                (i / width).checked_add_signed(dz),
            ) else {
                continue;
            };
            if x >= width || z >= depth {
                continue;
            }
            let j = z * width + x;
            let Some(q) = sites[j] else { continue };
            let next = cost + if dx == 0 || dz == 0 { 10 } else { 14 };
            if next >= costs[j] || !clear(barriers, p, q, 0.55, 1) {
                continue;
            }
            charge()?;
            if survey.crosses(p, q, 0) {
                costs[j] = next;
                parent[j] = i;
                open.push(std::cmp::Reverse((next + estimate(j), next, j)));
            }
        }
    }
    if parent[goal] == usize::MAX {
        return Ok(None);
    }
    let mut chain = vec![b];
    let mut i = goal;
    while i != start {
        i = parent[i];
        chain.push(sites[i].unwrap());
    }
    chain.reverse();
    let mut via = Vec::new();
    let mut first = 0;
    while first + 1 < chain.len() {
        let mut next = chain.len() - 1;
        while next > first + 1 {
            charge()?;
            if clear(barriers, chain[first], chain[next], 0.55, 1)
                && survey.crosses(chain[first], chain[next], 0)
            {
                break;
            }
            next -= 1;
        }
        if next + 1 < chain.len() {
            via.push(chain[next]);
        }
        first = next;
    }
    Ok(Some(via))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::nav::swbf2::Swbf2Hub;

    struct Deck;

    impl Deck {
        fn on_deck(p: Vec3) -> bool {
            (p.z.abs() <= 1. && (p.x <= 5. || p.x >= 15.))
                || ((p.x - 5.).abs() <= 1. && (-1. ..=8.).contains(&p.z))
                || ((p.z - 7.).abs() <= 1. && (4. ..=16.).contains(&p.x))
                || ((p.x - 15.).abs() <= 1. && (-1. ..=8.).contains(&p.z))
        }
    }
    impl Survey for Deck {
        fn floors(&mut self, _: &Swbf2Hub, _: &mut Vec<Vec3>) {}
        fn floor(&mut self, p: Vec3) -> Option<Vec3> {
            Self::on_deck(p).then_some(p)
        }
        fn crosses(&mut self, p: Vec3, q: Vec3, _: u8) -> bool {
            (0..=64).all(|i| {
                let t = f64::from(i) / 64.;
                Self::on_deck(Vec3::new(p.x + t * (q.x - p.x), 0., p.z + t * (q.z - p.z)))
            })
        }
    }
    #[test]
    fn doorway_survey_follows_both_turns_on_the_deck() {
        let mut barriers = BarrierSet::new(vec![]);
        let (start, door, end) = (
            Vec3::default(),
            Vec3::new(3., 0., 0.),
            Vec3::new(20., 0., 0.),
        );
        let path = deck_passage(&mut Deck, &mut barriers, start, door, end, &mut || Ok(()))
            .unwrap()
            .unwrap();
        let mut at = start;
        for &next in path.points().iter().chain(std::iter::once(&end)) {
            assert!(Deck.crosses(at, next, 0));
            at = next;
        }
        assert!(path.points().contains(&door));
    }
}
