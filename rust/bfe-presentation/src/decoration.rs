//! Continuous visual sources yield particle and draw capacity to the combat player.
use super::{d, dk, e, f, h, Presentation, NONE};

pub struct DecorationBudget {
    withheld: Vec<usize>,
}

impl DecorationBudget {
    pub fn new(capacity: usize) -> Self {
        Self {
            withheld: Vec::with_capacity(capacity),
        }
    }

    /// Step an independent set of emitters within the capacity left by combat.
    /// Handles, child effects, commands and birth events also have separate storage.
    pub fn reserve(&mut self, player: &mut Presentation, room: usize) {
        let capacity = player.particles.len();
        let limit = room.min(capacity);
        let occupied = capacity - player.free_particles.len();
        if occupied > limit {
            player.thin_population(occupied, occupied - limit);
        }
        let free = player.free_particles.len();
        self.withheld
            .extend(player.free_particles.drain(free - (capacity - limit)..));
    }

    pub fn release(&mut self, player: &mut Presentation) {
        player.free_particles.append(&mut self.withheld);
        player.header[h::PARTICLES] = (player.particles.len() - player.free_particles.len()) as f64;
    }
}

impl Presentation {
    /// Slow the birth clock of a mounted source while its particles continue to
    /// move and age in real seconds. Child effects keep their authored timing.
    pub fn mount_rate(&mut self, id: usize, generation: u32, rate: f64, dt: f64) {
        let Some(&slot) = self.lookup.get(id).filter(|&&slot| slot != NONE) else {
            return;
        };
        let effect = &self.effects[slot];
        if effect.generation != generation || effect.stopped || rate >= 1. {
            return;
        }
        let pause = dt * (1. - rate.clamp(0., 1.));
        let mut em = effect.first;
        while em != NONE {
            self.emitters[em].next_burst += pause;
            em = self.emitters[em].next;
        }
    }

    // Combat can take the entire pool in a single tick. Retire a proportional
    // sample across the existing trails, including their carrier/child links.
    fn thin_population(&mut self, occupied: usize, excess: usize) {
        let mut fraction = 0;
        for i in 0..self.order.len() {
            let mut em = self.effects[self.order[i]].first;
            while em != NONE {
                let mut particle = self.emitters[em].first;
                let mut last = NONE;
                while particle != NONE {
                    let p = self.particles[particle];
                    fraction += excess;
                    if fraction >= occupied {
                        fraction -= occupied;
                        if last == NONE {
                            self.emitters[em].first = p.next;
                        } else {
                            self.particles[last].next = p.next;
                        }
                        if p.child != NONE && self.effects[p.child].token == p.child_token {
                            self.effects[p.child].stopped = true;
                        }
                        self.free_particles.push(particle);
                    } else {
                        last = particle;
                    }
                    particle = p.next;
                }
                self.emitters[em].last = last;
                em = self.emitters[em].next;
            }
        }
    }

    /// Merge the two publications in combat-first order. Analytic particles
    /// index the renderer's owner rows. Model and evaluated rows are independent.
    pub fn append_decoration(&mut self, decor: &Self) {
        let base = self.header[h::EFFECTS] as usize;
        let owners =
            (decor.header[h::EFFECTS] as usize).min(self.effects.len().saturating_sub(base));
        if owners > 0 {
            self.effect_output[base * e::STRIDE..(base + owners) * e::STRIDE]
                .copy_from_slice(&decor.effect_output[..owners * e::STRIDE]);
            self.header[h::EFFECTS] += owners as f64;
        }
        let mut count = self.header[h::DRAWS] as usize;
        for row in decor
            .draws
            .chunks_exact(d::STRIDE)
            .take(decor.header[h::DRAWS] as usize)
        {
            if count >= self.particles.len() {
                break;
            }
            let analytic = row[d::KIND] == dk::ANALYTIC as f64;
            if analytic && row[d::EFFECT] as usize >= owners {
                continue;
            }
            let out = &mut self.draws[count * d::STRIDE..][..d::STRIDE];
            out.copy_from_slice(row);
            // Evaluated trail points use the owner identity to join adjacent samples.
            out[d::EFFECT] += base as f64;
            count += 1;
        }
        self.header[h::DRAWS] = count as f64;
        self.header[h::PARTICLES] += decor.header[h::PARTICLES];
    }

    pub fn particle_capacity(&self) -> usize {
        self.particles.len()
    }

    /// Copy the display clocks. Each player owns its command batch.
    pub fn copy_clock(&mut self, other: &Self) {
        self.input[..f::STRIDE].copy_from_slice(&other.input[..f::STRIDE]);
        self.input[f::COMMANDS] = 0.;
    }
}
