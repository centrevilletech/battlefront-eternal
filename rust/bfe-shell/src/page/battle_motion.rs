//! Page spawn entrance and class changes over retained geometry. The same
//! transforms move its published hit rectangles, so clicks follow the panels.
use super::{motion, Page};
use bfe_battle_ui::spawn::SpawnHitTarget;
use bfe_render::interface::Batch;
pub(super) enum Group {
    Head,
    Roster,
    Class(usize, bool),
    Tactical,
    Caption,
    Spawn,
}
pub(super) struct Span {
    pub first: usize,
    pub end: usize,
    pub hits: [usize; 2],
    pub group: Group,
}
pub(super) struct BattlePaintState {
    pub spawn: Vec<Span>,
}
impl Default for BattlePaintState {
    fn default() -> Self {
        Self {
            spawn: Vec::with_capacity(16),
        }
    }
}
impl Page {
    #[allow(clippy::too_many_arguments)]
    pub fn animate_spawn(
        &self,
        b: &mut Batch,
        hits: &mut [([f64; 4], SpawnHitTarget)],
        age: f64,
        class_age: f64,
        team_age: f64,
        w: f64,
        h: f64,
        ih: f64,
        cursor: [f64; 2],
    ) {
        for span in &self.battle.spawn {
            let (delay, duration, from, clock) = match span.group {
                Group::Head => (0.15, 0.6, [0., -h * 0.03], age),
                Group::Roster => (0.3, 0.6, [-w * 0.04, 0.], age),
                Group::Class(i, _) => {
                    if age < 1.3 {
                        (0.35 + i as f64 * 0.06, 0.5, [-w * 0.03, 0.], age)
                    } else {
                        (i as f64 * 0.06, 0.5, [-w * 0.03, 0.], team_age)
                    }
                }
                Group::Tactical => (0.3, 0.6, [w * 0.04, 0.], age),
                Group::Caption => {
                    if age < 1.3 {
                        (0.6, 0.7, [0., h * 0.012], age)
                    } else {
                        (0., 0.5, [0., h * 0.012], class_age)
                    }
                }
                Group::Spawn => (0.6, 0.7, [0., h * 0.012], age),
            };
            let progress = ((clock - delay) / duration).clamp(0., 1.);
            let eased = if progress >= 1. {
                1.
            } else {
                motion::ease(progress, 0.2, 0.8, 0.2, 1.)
            };
            let mut offset = [from[0] * (1. - eased), from[1] * (1. - eased)];
            let mut opacity = eased * (age / 0.2).clamp(0., 1.);
            if let Group::Class(_, picked) = span.group {
                let hovered = hits.get(span.hits[0]).is_some_and(|(r, _)| {
                    cursor[0] >= r[0] && cursor[0] <= r[2] && cursor[1] >= r[1] && cursor[1] <= r[3]
                });
                if !picked {
                    opacity *= if hovered { 1. } else { 0.82 };
                    if hovered {
                        offset[0] += 4.;
                    }
                }
            }
            for (r, _) in hits.iter_mut().take(span.hits[1]).skip(span.hits[0]) {
                r[0] += offset[0] * 1024. / w;
                r[2] += offset[0] * 1024. / w;
                r[1] += offset[1] * ih / h;
                r[3] += offset[1] * ih / h;
            }
            if opacity >= 1. && offset == [0.; 2] {
                continue;
            }
            for q in span.first..span.end {
                for vertex in 0..4 {
                    let p = q * 12 + vertex * 3;
                    b.position[p] += (offset[0] * 1024. / w) as f32;
                    b.position[p + 1] -= (offset[1] * 768. / h) as f32;
                    b.tint[q * 16 + vertex * 4 + 3] *= opacity as f32;
                }
            }
            b.start = b.start.min(span.first * 4);
            b.end = b.end.max(span.end * 4);
        }
    }
}
